use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, RuntimeExecutableRequest,
};

use crate::{
    method_dispatch::plan_object_method_dispatch, object_lifecycle::aidl_object_live,
    TunerServiceRuntime,
};

pub type SharedObjectMethodRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMethodTxnTarget {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
}

impl ObjectMethodTxnTarget {
    pub const fn new(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
    ) -> Self {
        Self {
            object_id,
            generation,
            object_kind,
        }
    }

    pub const fn object_id(self) -> AidlObjectId {
        self.object_id
    }

    pub const fn generation(self) -> AidlObjectGeneration {
        self.generation
    }

    pub const fn object_kind(self) -> AidlObjectKind {
        self.object_kind
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectMethodTxnPlan {
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
}

impl ObjectMethodTxnPlan {
    fn new(
        command_plan: CommandPlan,
        executable_request: Option<RuntimeExecutableRequest>,
    ) -> Self {
        Self {
            command_plan,
            executable_request,
        }
    }

    pub const fn command_plan(&self) -> CommandPlan {
        self.command_plan
    }

    pub fn executable_request(&self) -> Option<RuntimeExecutableRequest> {
        self.executable_request.clone()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectMethodDispatchPreflight {
    state: ObjectMethodDispatchPreflightState,
}

#[derive(Debug, Eq, PartialEq)]
enum ObjectMethodDispatchPreflightState {
    AlreadyPlanned(ObjectMethodDispatchPreflightProof),
}

#[derive(Debug, Eq, PartialEq)]
struct ObjectMethodDispatchPreflightProof {
    _private: (),
}

impl ObjectMethodDispatchPreflight {
    const fn already_planned() -> Self {
        Self {
            state: ObjectMethodDispatchPreflightState::AlreadyPlanned(
                ObjectMethodDispatchPreflightProof { _private: () },
            ),
        }
    }

    pub(crate) fn plan(self, _runtime: &mut TunerServiceRuntime) -> Result<(), HalError> {
        match self.state {
            ObjectMethodDispatchPreflightState::AlreadyPlanned(_) => Ok(()),
        }
    }
}

#[derive(Debug)]
pub enum ObjectMethodTxnBuildError<E> {
    Runtime(HalError),
    Builder(E),
}

pub fn build_and_plan_object_method_request_after_live<T, E, F>(
    runtime: &SharedObjectMethodRuntime,
    target: ObjectMethodTxnTarget,
    build: F,
) -> Result<(ObjectMethodTxnPlan, ObjectMethodDispatchPreflight, T), ObjectMethodTxnBuildError<E>>
where
    F: FnOnce() -> Result<(CommandPlan, Option<RuntimeExecutableRequest>, T), E>,
{
    let mut runtime = runtime.lock().map_err(|_| {
        ObjectMethodTxnBuildError::Runtime(HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        ))
    })?;
    aidl_object_live(
        &runtime,
        target.object_id(),
        target.generation(),
        target.object_kind(),
    )
    .map_err(ObjectMethodTxnBuildError::Runtime)?;
    let (command_plan, executable_request, request) =
        build().map_err(ObjectMethodTxnBuildError::Builder)?;
    let plan = ObjectMethodTxnPlan::new(command_plan, executable_request);
    if plan.command_plan().object() != target.object_kind() {
        return Err(ObjectMethodTxnBuildError::Runtime(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL method/object kind mismatch",
        )));
    }
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
    Ok((
        plan,
        ObjectMethodDispatchPreflight::already_planned(),
        request,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeOwnerRelation};
    use maleicacid_tuner_hal2_domain_request::{
        AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
        RuntimeExecutableRequest,
    };
    use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

    fn target() -> ObjectMethodTxnTarget {
        ObjectMethodTxnTarget::new(
            AidlObjectId(701),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
    }

    fn runtime_with_live_filter() -> SharedObjectMethodRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(701),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(701),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        Arc::new(Mutex::new(runtime))
    }

    fn filter_get_id_plan() -> ObjectMethodTxnPlan {
        ObjectMethodTxnPlan::new(
            CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterGetId).unwrap(),
            Some(RuntimeExecutableRequest::NoPayload),
        )
    }

    #[test]
    fn object_method_dispatch_preflight_required_runs_dispatch() {
        let runtime = runtime_with_live_filter();
        let mut guard = runtime.lock().expect("lock succeeds");
        plan_object_method_dispatch(
            &mut guard,
            filter_get_id_plan().command_plan(),
            filter_get_id_plan().executable_request(),
        )
        .expect("required dispatch preflight succeeds");
    }

    #[test]
    fn object_method_request_builder_plans_dispatch_under_runtime_lock() {
        let runtime = runtime_with_live_filter();
        let runtime_for_builder = runtime.clone();

        let (plan, preflight, value) =
            build_and_plan_object_method_request_after_live::<_, (), _>(&runtime, target(), || {
                assert!(runtime_for_builder.try_lock().is_err());
                let plan = filter_get_id_plan();
                Ok((plan.command_plan(), plan.executable_request(), 7))
            })
            .expect("builder and dispatch planning succeed");

        assert_eq!(plan.command_plan().api(), AidlApi::FilterGetId);
        assert_eq!(preflight, ObjectMethodDispatchPreflight::already_planned());
        assert_eq!(value, 7);
    }

    #[test]
    fn object_method_request_builder_failure_skips_dispatch_preflight() {
        let runtime = runtime_with_live_filter();

        let result =
            build_and_plan_object_method_request_after_live::<(), _, _>(&runtime, target(), || {
                Err("builder failed")
            });

        assert!(matches!(
            result,
            Err(ObjectMethodTxnBuildError::Builder("builder failed"))
        ));
    }

    #[test]
    fn object_method_request_builder_rejects_method_kind_mismatch() {
        let runtime = runtime_with_live_filter();

        let result =
            build_and_plan_object_method_request_after_live::<_, (), _>(&runtime, target(), || {
                Ok((
                    CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSetVoltage).unwrap(),
                    Some(RuntimeExecutableRequest::NoPayload),
                    (),
                ))
            });

        assert!(matches!(result, Err(ObjectMethodTxnBuildError::Runtime(_))));
    }

    #[test]
    fn object_method_request_builder_rejects_closed_before_builder() {
        let runtime = runtime_with_live_filter();
        {
            let mut guard = runtime.lock().expect("lock succeeds");
            guard
                .object_table_mut()
                .begin_close_cascade(
                    AidlObjectId(701),
                    AidlObjectGeneration(1),
                    CleanupStep::UnregisterRuntime,
                )
                .expect("begin close succeeds");
            guard
                .object_table_mut()
                .commit_close_cascade(AidlObjectId(701), AidlObjectGeneration(1))
                .expect("commit close succeeds");
        }

        let mut builder_called = false;
        let result =
            build_and_plan_object_method_request_after_live::<_, (), _>(&runtime, target(), || {
                builder_called = true;
                let plan = filter_get_id_plan();
                Ok((plan.command_plan(), plan.executable_request(), ()))
            });

        assert!(matches!(result, Err(ObjectMethodTxnBuildError::Runtime(_))));
        assert!(!builder_called);
    }
}
