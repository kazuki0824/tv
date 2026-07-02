use std::sync::{Arc, Mutex};

use crate::registry::LnbRegistryProfile;
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_device::{FrontendRuntimeState, FrontendSignalState};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
    RuntimeExecutableRequest,
};

use crate::{
    method_dispatch::plan_object_method_dispatch, object_lifecycle::aidl_object_live,
    TunerServiceRuntime,
};

pub type SharedObjectMethodRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectMethodTxnTarget {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
}

impl ObjectMethodTxnTarget {
    const fn new(
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectQueryRequest {
    FilterGetQueueDesc,
    FilterGetId,
    FilterGetId64Bit,
    DvrGetQueueDesc,
    FrontendGetStatus {
        status_types: Vec<ObjectFrontendStatusType>,
    },
    FrontendGetFrontendStatusReadiness {
        status_types: Vec<ObjectFrontendStatusType>,
    },
    DemuxGetAvSyncHwId {
        filter_object_id: AidlObjectId,
        filter_generation: AidlObjectGeneration,
    },
    DemuxGetAvSyncTime {
        av_sync_hw_id: i32,
    },
}

impl ObjectQueryRequest {
    fn method(&self) -> AidlMethodCall {
        match self {
            Self::FilterGetQueueDesc => AidlMethodCall::FilterGetQueueDesc,
            Self::FilterGetId => AidlMethodCall::FilterGetId,
            Self::FilterGetId64Bit => AidlMethodCall::FilterGetId64Bit,
            Self::DvrGetQueueDesc => AidlMethodCall::DvrGetQueueDesc,
            Self::FrontendGetStatus { .. } => AidlMethodCall::PublicApi {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendGetStatus,
            },
            Self::FrontendGetFrontendStatusReadiness { .. } => AidlMethodCall::PublicApi {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendGetFrontendStatusReadiness,
            },
            Self::DemuxGetAvSyncHwId { .. } => AidlMethodCall::PublicApi {
                object: AidlObjectKind::Demux,
                api: AidlApi::DemuxGetAvSyncHwId,
            },
            Self::DemuxGetAvSyncTime { .. } => AidlMethodCall::PublicApi {
                object: AidlObjectKind::Demux,
                api: AidlApi::DemuxGetAvSyncTime,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFrontendStatusType {
    DemodLock,
    LnbVoltage,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFrontendStatusValue {
    DemodLocked(bool),
    LnbVoltageNone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFrontendStatusReadinessValue {
    Stable,
    Unstable,
    Unavailable,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectFrontendStatusSnapshot {
    pub lnb_profile: Option<LnbRegistryProfile>,
    pub runtime_state: FrontendRuntimeState,
    pub signal_state: FrontendSignalState,
}

fn lnb_profile_supports_voltage_status(profile: Option<LnbRegistryProfile>) -> bool {
    matches!(
        profile,
        Some(LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb)
    )
}

fn object_frontend_status_value(
    snapshot: ObjectFrontendStatusSnapshot,
    status_type: ObjectFrontendStatusType,
) -> Option<ObjectFrontendStatusValue> {
    match status_type {
        ObjectFrontendStatusType::DemodLock => Some(ObjectFrontendStatusValue::DemodLocked(
            matches!(snapshot.signal_state, FrontendSignalState::Locked),
        )),
        ObjectFrontendStatusType::LnbVoltage
            if lnb_profile_supports_voltage_status(snapshot.lnb_profile) =>
        {
            Some(ObjectFrontendStatusValue::LnbVoltageNone)
        }
        ObjectFrontendStatusType::LnbVoltage | ObjectFrontendStatusType::Unsupported => None,
    }
}

fn object_frontend_readiness_value(
    snapshot: ObjectFrontendStatusSnapshot,
    status_type: ObjectFrontendStatusType,
) -> ObjectFrontendStatusReadinessValue {
    if matches!(status_type, ObjectFrontendStatusType::LnbVoltage)
        && !lnb_profile_supports_voltage_status(snapshot.lnb_profile)
    {
        return ObjectFrontendStatusReadinessValue::Unsupported;
    }
    if matches!(status_type, ObjectFrontendStatusType::Unsupported) {
        return ObjectFrontendStatusReadinessValue::Unsupported;
    }
    match snapshot.runtime_state {
        FrontendRuntimeState::Idle => ObjectFrontendStatusReadinessValue::Stable,
        FrontendRuntimeState::Tuning { .. } | FrontendRuntimeState::Scanning { .. } => {
            match snapshot.signal_state {
                FrontendSignalState::Locked => ObjectFrontendStatusReadinessValue::Stable,
                FrontendSignalState::NoSignal
                | FrontendSignalState::SignalDetected
                | FrontendSignalState::Unknown => ObjectFrontendStatusReadinessValue::Unstable,
            }
        }
        FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
            ObjectFrontendStatusReadinessValue::Unavailable
        }
    }
}

#[derive(Debug)]
pub enum ObjectQueryResponse {
    QueueDescriptor(maleicacid_tuner_hal2_demux::QueueDescriptorSnapshot),
    PublicId(i32),
    PublicId64(i64),
    FrontendStatus(Vec<ObjectFrontendStatusValue>),
    FrontendStatusReadiness(Vec<ObjectFrontendStatusReadinessValue>),
    AvSyncHwId(i32),
    AvSyncTime(i64),
}

fn execute_object_query_request(
    query: &crate::boot::RuntimeQuery<'_>,
    target: ObjectMethodTxnTarget,
    request: ObjectQueryRequest,
) -> Result<ObjectQueryResponse, HalError> {
    match request {
        ObjectQueryRequest::FilterGetQueueDesc => query
            .filter_queue_descriptor_snapshot_for_aidl_object(
                target.object_id(),
                target.generation(),
            )
            .map(ObjectQueryResponse::QueueDescriptor),
        ObjectQueryRequest::FilterGetId => query
            .public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Filter,
            )
            .map(ObjectQueryResponse::PublicId),
        ObjectQueryRequest::FilterGetId64Bit => query
            .public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Filter,
            )
            .map(i64::from)
            .map(ObjectQueryResponse::PublicId64),
        ObjectQueryRequest::DvrGetQueueDesc => query
            .dvr_queue_descriptor_snapshot_for_aidl_object(target.object_id(), target.generation())
            .map(ObjectQueryResponse::QueueDescriptor),
        ObjectQueryRequest::FrontendGetStatus { status_types } => {
            let snapshot = query
                .frontend_status_query_for_aidl_object(target.object_id(), target.generation())?;
            Ok(ObjectQueryResponse::FrontendStatus(
                status_types
                    .into_iter()
                    .filter_map(|status_type| object_frontend_status_value(snapshot, status_type))
                    .collect(),
            ))
        }
        ObjectQueryRequest::FrontendGetFrontendStatusReadiness { status_types } => {
            let snapshot = query
                .frontend_status_query_for_aidl_object(target.object_id(), target.generation())?;
            Ok(ObjectQueryResponse::FrontendStatusReadiness(
                status_types
                    .into_iter()
                    .map(|status_type| object_frontend_readiness_value(snapshot, status_type))
                    .collect(),
            ))
        }
        ObjectQueryRequest::DemuxGetAvSyncHwId {
            filter_object_id,
            filter_generation,
        } => {
            let filter_entry = query.public_entry_for_object_method(
                filter_object_id,
                filter_generation,
                AidlObjectKind::Filter,
            )?;
            let owner_object_id = match filter_entry.owner() {
                crate::RuntimeOwnerRelation::Demux { demux, .. } => demux,
                _ => {
                    return Err(HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "filter owner demux is not live",
                    ))
                }
            };
            if owner_object_id != target.object_id() {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "filter does not belong to this demux",
                ));
            }
            query.ensure_media_filter_for_demux_object(
                target.object_id(),
                target.generation(),
                filter_object_id,
                filter_generation,
            )?;
            query
                .first_pcr_filter_id_for_demux_object(target.object_id(), target.generation())?
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "no live PCR filter is associated with this demux",
                    )
                })
                .map(ObjectQueryResponse::AvSyncHwId)
        }
        ObjectQueryRequest::DemuxGetAvSyncTime { av_sync_hw_id } => {
            if av_sync_hw_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV sync hardware id must be non-negative",
                ));
            }
            if !query.is_live_pcr_filter_for_demux_object(
                target.object_id(),
                target.generation(),
                av_sync_hw_id,
            )? {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV sync hardware id must refer to a live PCR filter owned by this demux",
                ));
            }
            Ok(ObjectQueryResponse::AvSyncTime(0))
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ObjectMethodTxnPlan {
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

    const fn command_plan(&self) -> CommandPlan {
        self.command_plan
    }
    fn executable_request(&self) -> Option<RuntimeExecutableRequest> {
        self.executable_request.clone()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ObjectMethodDispatchProof {
    target: ObjectMethodTxnTarget,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectMethodExecutionToken {
    target: ObjectMethodTxnTarget,
}

impl ObjectMethodExecutionToken {
    fn new(target: ObjectMethodTxnTarget) -> Self {
        Self { target }
    }

    pub(crate) fn consume_for_object(
        self,
        _runtime: &mut TunerServiceRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
    ) -> Result<(), HalError> {
        if self.target == ObjectMethodTxnTarget::new(object_id, generation, object_kind) {
            aidl_object_live(_runtime, object_id, generation, object_kind)
        } else {
            Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "object method execution token target mismatch",
            ))
        }
    }
}

impl ObjectMethodDispatchProof {
    const fn new(target: ObjectMethodTxnTarget) -> Self {
        Self { target }
    }

    fn consume_for_target(self, target: ObjectMethodTxnTarget) -> Result<(), HalError> {
        if self.target == target {
            Ok(())
        } else {
            Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "dispatch proof target does not match object method target",
            ))
        }
    }
}

#[derive(Debug)]
pub enum ObjectMethodTxnBuildError<E> {
    Runtime(HalError),
    Builder(E),
}

fn build_plan(
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
) -> ObjectMethodTxnPlan {
    ObjectMethodTxnPlan::new(command_plan, executable_request)
}

fn validate_plan_target(
    plan: &ObjectMethodTxnPlan,
    target: ObjectMethodTxnTarget,
) -> Result<(), HalError> {
    if plan.command_plan().object() != target.object_kind() {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL method/object kind mismatch",
        ));
    }
    Ok(())
}

fn plan_aidl_method_call(method: AidlMethodCall) -> Result<ObjectMethodTxnPlan, HalError> {
    let method_plan = AidlMethodAdapter::plan(method)?;
    Ok(build_plan(
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
    ))
}

fn build_aidl_method_plan_after_live_inner<T, E, F>(
    runtime: &SharedObjectMethodRuntime,
    target: ObjectMethodTxnTarget,
    build: F,
) -> Result<(ObjectMethodTxnPlan, T), ObjectMethodTxnBuildError<E>>
where
    F: FnOnce() -> Result<(AidlMethodCall, T), E>,
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
    let (method, request) = build().map_err(ObjectMethodTxnBuildError::Builder)?;
    let plan = plan_aidl_method_call(method).map_err(ObjectMethodTxnBuildError::Runtime)?;
    validate_plan_target(&plan, target).map_err(ObjectMethodTxnBuildError::Runtime)?;
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
    Ok((plan, request))
}

pub fn execute_object_query_call_after_live(
    runtime: &SharedObjectMethodRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    request: ObjectQueryRequest,
) -> Result<ObjectQueryResponse, HalError> {
    let target = ObjectMethodTxnTarget::new(object_id, generation, object_kind);
    let mut runtime = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        )
    })?;
    aidl_object_live(
        &runtime,
        target.object_id(),
        target.generation(),
        target.object_kind(),
    )?;
    let method = request.method();
    let plan = plan_aidl_method_call(method)?;
    validate_plan_target(&plan, target)?;
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())?;
    let query = runtime.query();
    execute_object_query_request(&query, target, request)
}

pub fn execute_object_query_call_after_live_with_aidl_input_conversion<E, Build>(
    runtime: &SharedObjectMethodRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    method: AidlMethodCall,
    build: Build,
) -> Result<ObjectQueryResponse, ObjectMethodTxnBuildError<E>>
where
    Build: FnOnce() -> Result<ObjectQueryRequest, E>,
{
    let target = ObjectMethodTxnTarget::new(object_id, generation, object_kind);
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
    let plan = plan_aidl_method_call(method).map_err(ObjectMethodTxnBuildError::Runtime)?;
    validate_plan_target(&plan, target).map_err(ObjectMethodTxnBuildError::Runtime)?;
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
    let request = build().map_err(ObjectMethodTxnBuildError::Builder)?;
    let query = runtime.query();
    execute_object_query_request(&query, target, request)
        .map_err(ObjectMethodTxnBuildError::Runtime)
}

pub fn execute_object_method_call_after_live<T, E, B, Build, Execute>(
    runtime: &SharedObjectMethodRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    build: Build,
    execute: Execute,
) -> Result<T, ObjectMethodTxnBuildError<E>>
where
    Build: FnOnce() -> Result<(AidlMethodCall, B), E>,
    Execute: FnOnce(&mut TunerServiceRuntime, ObjectMethodExecutionToken, B) -> Result<T, HalError>,
{
    let target = ObjectMethodTxnTarget::new(object_id, generation, object_kind);
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
    let (method, request) = build().map_err(ObjectMethodTxnBuildError::Builder)?;
    let plan = plan_aidl_method_call(method).map_err(ObjectMethodTxnBuildError::Runtime)?;
    validate_plan_target(&plan, target).map_err(ObjectMethodTxnBuildError::Runtime)?;
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
    ObjectMethodDispatchProof::new(target)
        .consume_for_target(target)
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
    execute(
        &mut runtime,
        ObjectMethodExecutionToken::new(target),
        request,
    )
    .map_err(ObjectMethodTxnBuildError::Runtime)
}

pub fn execute_shared_object_method_call_after_live<T, E, B, Build, Execute>(
    runtime: &SharedObjectMethodRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    build: Build,
    execute: Execute,
) -> Result<T, ObjectMethodTxnBuildError<E>>
where
    Build: FnOnce() -> Result<(AidlMethodCall, B), E>,
    Execute:
        FnOnce(SharedObjectMethodRuntime, ObjectMethodExecutionToken, B) -> Result<T, HalError>,
{
    let target = ObjectMethodTxnTarget::new(object_id, generation, object_kind);
    let request = {
        let mut runtime_guard = runtime.lock().map_err(|_| {
            ObjectMethodTxnBuildError::Runtime(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            ))
        })?;
        aidl_object_live(
            &runtime_guard,
            target.object_id(),
            target.generation(),
            target.object_kind(),
        )
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
        let (method, request) = build().map_err(ObjectMethodTxnBuildError::Builder)?;
        let plan = plan_aidl_method_call(method).map_err(ObjectMethodTxnBuildError::Runtime)?;
        validate_plan_target(&plan, target).map_err(ObjectMethodTxnBuildError::Runtime)?;
        plan_object_method_dispatch(
            &mut runtime_guard,
            plan.command_plan(),
            plan.executable_request(),
        )
        .map_err(ObjectMethodTxnBuildError::Runtime)?;
        ObjectMethodDispatchProof::new(target)
            .consume_for_target(target)
            .map_err(ObjectMethodTxnBuildError::Runtime)?;
        request
    };
    execute(
        Arc::clone(runtime),
        ObjectMethodExecutionToken::new(target),
        request,
    )
    .map_err(ObjectMethodTxnBuildError::Runtime)
}

pub fn preflight_object_method_after_live_plan_only<E, F>(
    runtime: &SharedObjectMethodRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    build: F,
) -> Result<AidlApi, ObjectMethodTxnBuildError<E>>
where
    F: FnOnce() -> Result<AidlMethodCall, E>,
{
    let target = ObjectMethodTxnTarget::new(object_id, generation, object_kind);
    let (plan, ()) = build_aidl_method_plan_after_live_inner(runtime, target, || {
        build().map(|method| (method, ()))
    })?;
    Ok(plan.command_plan().api())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(
        lnb_profile: Option<LnbRegistryProfile>,
        runtime_state: FrontendRuntimeState,
        signal_state: FrontendSignalState,
    ) -> ObjectFrontendStatusSnapshot {
        ObjectFrontendStatusSnapshot {
            lnb_profile,
            runtime_state,
            signal_state,
        }
    }

    #[test]
    fn frontend_status_value_uses_dto_snapshot_without_registry_entry() {
        let locked = snapshot(
            None,
            FrontendRuntimeState::Idle,
            FrontendSignalState::Locked,
        );
        let unlocked = snapshot(
            None,
            FrontendRuntimeState::Idle,
            FrontendSignalState::NoSignal,
        );

        assert_eq!(
            object_frontend_status_value(locked, ObjectFrontendStatusType::DemodLock),
            Some(ObjectFrontendStatusValue::DemodLocked(true))
        );
        assert_eq!(
            object_frontend_status_value(unlocked, ObjectFrontendStatusType::DemodLock),
            Some(ObjectFrontendStatusValue::DemodLocked(false))
        );
    }

    #[test]
    fn frontend_readiness_reports_unsupported_lnb_voltage_from_dto_snapshot() {
        let value = object_frontend_readiness_value(
            snapshot(
                None,
                FrontendRuntimeState::Idle,
                FrontendSignalState::Locked,
            ),
            ObjectFrontendStatusType::LnbVoltage,
        );

        assert_eq!(value, ObjectFrontendStatusReadinessValue::Unsupported);
    }
}
