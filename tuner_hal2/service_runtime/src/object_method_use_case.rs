use std::sync::{Arc, Mutex};

use crate::registry::LnbRegistryProfile;
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_device::{FrontendRuntimeState, FrontendSignalState};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
    RuntimeExecutableRequest,
};

use crate::{
    boot::{map_queue_descriptor_query_error, QueueDescriptorExportPlan},
    diagnostics::QueueDescriptorQueryDiagnosticRecord,
    method_dispatch::plan_object_method_dispatch,
    object_lifecycle::aidl_object_live,
    TunerServiceRuntime,
};
use maleicacid_tuner_hal2_demux::QueueDescriptorQueryError;

const TUNER_INVALID_TIMESTAMP: i64 = -1;

pub type SharedObjectMethodRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectMethodUseCaseTarget {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
}

impl ObjectMethodUseCaseTarget {
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
    FrontendGetHardwareInfo,
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
            Self::FrontendGetHardwareInfo => AidlMethodCall::PublicApi {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendGetHardwareInfo,
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
    RfLock,
    LnbVoltage,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFrontendStatusValue {
    DemodLocked(bool),
    RfLocked(bool),
    LnbVoltageNone,
    LnbVoltage11V,
    LnbVoltage15V,
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
    pub backend: FrontendBackendKind,
    pub lnb_profile: Option<LnbRegistryProfile>,
    pub runtime_state: FrontendRuntimeState,
    pub signal_state: FrontendSignalState,
    pub lnb_voltage: Option<maleicacid_tuner_hal2_lnb::LnbVoltage>,
}

pub fn lnb_profile_supports_voltage_status(profile: Option<LnbRegistryProfile>) -> bool {
    matches!(
        profile,
        Some(LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb)
    )
}

fn object_frontend_status_value(
    snapshot: ObjectFrontendStatusSnapshot,
    status_type: ObjectFrontendStatusType,
) -> Result<ObjectFrontendStatusValue, HalError> {
    if matches!(
        snapshot.runtime_state,
        FrontendRuntimeState::Closing | FrontendRuntimeState::Failed
    ) {
        return Err(HalError::Unsupported(
            "frontend status snapshot is unavailable after a fatal runtime transition",
        ));
    }
    match status_type {
        ObjectFrontendStatusType::DemodLock => {
            Ok(ObjectFrontendStatusValue::DemodLocked(matches!(
                snapshot.signal_state,
                FrontendSignalState::Locked
            )))
        }
        ObjectFrontendStatusType::RfLock if snapshot.backend == FrontendBackendKind::LinuxDvb => {
            Ok(ObjectFrontendStatusValue::RfLocked(matches!(
                snapshot.signal_state,
                FrontendSignalState::SignalDetected | FrontendSignalState::Locked
            )))
        }
        ObjectFrontendStatusType::RfLock => Err(HalError::Unsupported(
            "frontend RF lock status is unsupported",
        )),
        ObjectFrontendStatusType::LnbVoltage
            if lnb_profile_supports_voltage_status(snapshot.lnb_profile) =>
        {
            match snapshot.lnb_voltage {
                Some(maleicacid_tuner_hal2_lnb::LnbVoltage::None) => {
                    Ok(ObjectFrontendStatusValue::LnbVoltageNone)
                }
                Some(maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage11V) => {
                    Ok(ObjectFrontendStatusValue::LnbVoltage11V)
                }
                Some(maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V) => {
                    Ok(ObjectFrontendStatusValue::LnbVoltage15V)
                }
                None => Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "advertised frontend LNB voltage has no committed state",
                )),
            }
        }
        ObjectFrontendStatusType::LnbVoltage => Err(HalError::Unsupported(
            "frontend LNB voltage status is unsupported",
        )),
        ObjectFrontendStatusType::Unsupported => {
            Err(HalError::Unsupported("frontend status type is unsupported"))
        }
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
    if matches!(status_type, ObjectFrontendStatusType::Unsupported)
        || matches!(status_type, ObjectFrontendStatusType::RfLock)
            && snapshot.backend != FrontendBackendKind::LinuxDvb
    {
        return ObjectFrontendStatusReadinessValue::Unsupported;
    }
    if matches!(
        snapshot.runtime_state,
        FrontendRuntimeState::Closing | FrontendRuntimeState::Failed
    ) {
        return ObjectFrontendStatusReadinessValue::Unavailable;
    }
    match status_type {
        ObjectFrontendStatusType::LnbVoltage => {
            if snapshot.lnb_voltage.is_some() {
                ObjectFrontendStatusReadinessValue::Stable
            } else {
                ObjectFrontendStatusReadinessValue::Unavailable
            }
        }
        ObjectFrontendStatusType::DemodLock | ObjectFrontendStatusType::RfLock => match snapshot.signal_state {
            FrontendSignalState::Locked | FrontendSignalState::NoSignal => {
                ObjectFrontendStatusReadinessValue::Stable
            }
            FrontendSignalState::SignalDetected | FrontendSignalState::Unknown => {
                ObjectFrontendStatusReadinessValue::Unstable
            }
        },
        ObjectFrontendStatusType::Unsupported => {
            ObjectFrontendStatusReadinessValue::Unsupported
        }
    }
}

#[derive(Debug)]
pub enum ObjectQueryResponse {
    QueueDescriptor(maleicacid_tuner_hal2_demux::QueueDescriptorSnapshot),
    PublicId(i32),
    PublicId64(i64),
    FrontendStatus(Vec<ObjectFrontendStatusValue>),
    FrontendHardwareInfo(String),
    FrontendStatusReadiness(Vec<ObjectFrontendStatusReadinessValue>),
    AvSyncHwId(i32),
    AvSyncTime(i64),
}

enum ObjectQueryExecution {
    Immediate(ObjectQueryResponse),
    QueueDescriptor(QueueDescriptorExportPlan),
}

fn prepare_object_query_request(
    query: &crate::boot::RuntimeQuery<'_>,
    target: ObjectMethodUseCaseTarget,
    request: ObjectQueryRequest,
) -> Result<ObjectQueryExecution, HalError> {
    match request {
        ObjectQueryRequest::FilterGetQueueDesc => query
            .filter_queue_descriptor_export_plan_for_aidl_object(
                target.object_id(),
                target.generation(),
            )
            .map(ObjectQueryExecution::QueueDescriptor),
        ObjectQueryRequest::FilterGetId => query
            .public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Filter,
            )
            .map(ObjectQueryResponse::PublicId)
            .map(ObjectQueryExecution::Immediate),
        ObjectQueryRequest::FilterGetId64Bit => query
            .public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Filter,
            )
            .map(i64::from)
            .map(ObjectQueryResponse::PublicId64)
            .map(ObjectQueryExecution::Immediate),
        ObjectQueryRequest::DvrGetQueueDesc => query
            .dvr_queue_descriptor_export_plan_for_aidl_object(
                target.object_id(),
                target.generation(),
            )
            .map(ObjectQueryExecution::QueueDescriptor),
        ObjectQueryRequest::FrontendGetStatus { status_types } => {
            let snapshot = query
                .frontend_status_query_for_aidl_object(target.object_id(), target.generation())?;
            Ok(ObjectQueryExecution::Immediate(
                ObjectQueryResponse::FrontendStatus(
                    status_types
                        .into_iter()
                        .filter(|status_type| {
                            matches!(status_type, ObjectFrontendStatusType::DemodLock)
                                || matches!(status_type, ObjectFrontendStatusType::RfLock)
                                    && snapshot.backend == FrontendBackendKind::LinuxDvb
                                || matches!(status_type, ObjectFrontendStatusType::LnbVoltage)
                                    && lnb_profile_supports_voltage_status(snapshot.lnb_profile)
                        })
                        .map(|status_type| object_frontend_status_value(snapshot, status_type))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            ))
        }
        ObjectQueryRequest::FrontendGetHardwareInfo => {
            let entry = query
                .frontend_entry_for_aidl_object(target.object_id(), target.generation())?;
            let hardware_info = entry.hardware_info();
            if hardware_info.is_empty() {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "advertised frontend has empty probe-derived hardware information",
                ));
            }
            Ok(ObjectQueryExecution::Immediate(
                ObjectQueryResponse::FrontendHardwareInfo(hardware_info),
            ))
        }
        ObjectQueryRequest::FrontendGetFrontendStatusReadiness { status_types } => {
            let snapshot = query
                .frontend_status_query_for_aidl_object(target.object_id(), target.generation())?;
            Ok(ObjectQueryExecution::Immediate(
                ObjectQueryResponse::FrontendStatusReadiness(
                    status_types
                        .into_iter()
                        .map(|status_type| object_frontend_readiness_value(snapshot, status_type))
                        .collect(),
                ),
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
            let filter_id = query.public_runtime_id_for_object_method(
                filter_object_id,
                filter_generation,
                AidlObjectKind::Filter,
            )?;
            let demux_id = query.public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Demux,
            )?;
            query
                .av_sync_hw_id_for_media_filter(demux_id, filter_id)
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "no live PCR filter is associated with this demux",
                    )
                })
                .map(ObjectQueryResponse::AvSyncHwId)
                .map(ObjectQueryExecution::Immediate)
        }
        ObjectQueryRequest::DemuxGetAvSyncTime { av_sync_hw_id } => {
            if av_sync_hw_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV sync hardware id must be non-negative",
                ));
            }
            let pcr_filter_id = query
                .pcr_filter_id_for_av_sync_hw_id_for_demux_object(
                    target.object_id(),
                    target.generation(),
                    av_sync_hw_id,
                )?
                .ok_or_else(|| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "AV sync hardware id is not owned by this demux",
                    )
                })?;
            if !query.is_live_pcr_filter_for_demux_object(
                target.object_id(),
                target.generation(),
                pcr_filter_id,
            )? {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV sync hardware id must refer to a live PCR filter owned by this demux",
                ));
            }
            let demux_id = query.public_runtime_id_for_object_method(
                target.object_id(),
                target.generation(),
                AidlObjectKind::Demux,
            )?;
            let timestamp = match query
                .pcr_clock_time_90khz_for_demux(demux_id, pcr_filter_id)
                .and_then(|value| i64::try_from(value).ok())
            {
                Some(timestamp) => timestamp,
                None => TUNER_INVALID_TIMESTAMP,
            };
            Ok(ObjectQueryExecution::Immediate(
                ObjectQueryResponse::AvSyncTime(timestamp),
            ))
        }
    }
}

fn finish_object_query_execution(
    runtime: &SharedObjectMethodRuntime,
    execution: ObjectQueryExecution,
) -> Result<ObjectQueryResponse, HalError> {
    match execution {
        ObjectQueryExecution::Immediate(response) => Ok(response),
        ObjectQueryExecution::QueueDescriptor(plan) => {
            finish_queue_descriptor_export(runtime, plan)
        }
    }
}

fn finish_queue_descriptor_export(
    runtime: &SharedObjectMethodRuntime,
    plan: QueueDescriptorExportPlan,
) -> Result<ObjectQueryResponse, HalError> {
    let object_kind = plan.object_kind();
    let object_id = plan.object_id();
    let generation = plan.generation();
    let runtime_id = plan.runtime_id();
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while exporting queue descriptor",
        )
    })?;
    aidl_object_live(&guard, object_id, generation, object_kind)?;
    match plan.export_descriptor() {
        Ok(snapshot) => Ok(ObjectQueryResponse::QueueDescriptor(snapshot)),
        Err(error) => {
            guard.record_queue_descriptor_query_diagnostic(
                QueueDescriptorQueryDiagnosticRecord::new(
                    object_kind,
                    object_id,
                    generation,
                    runtime_id,
                    error.clone(),
                ),
            );
            Err(map_queue_descriptor_query_error(
                QueueDescriptorQueryError::Runtime(error),
            ))
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ObjectMethodUseCasePlan {
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
}

impl ObjectMethodUseCasePlan {
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
    target: ObjectMethodUseCaseTarget,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectMethodExecutionToken {
    target: ObjectMethodUseCaseTarget,
}

impl ObjectMethodExecutionToken {
    fn new(target: ObjectMethodUseCaseTarget) -> Self {
        Self { target }
    }

    pub(crate) fn consume_for_object(
        self,
        _runtime: &mut TunerServiceRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
    ) -> Result<(), HalError> {
        if self.target == ObjectMethodUseCaseTarget::new(object_id, generation, object_kind) {
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
    const fn new(target: ObjectMethodUseCaseTarget) -> Self {
        Self { target }
    }

    fn consume_for_target(self, target: ObjectMethodUseCaseTarget) -> Result<(), HalError> {
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
pub enum ObjectMethodUseCaseBuildError<E> {
    Runtime(HalError),
    Builder(E),
}

fn build_plan(
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
) -> ObjectMethodUseCasePlan {
    ObjectMethodUseCasePlan::new(command_plan, executable_request)
}

fn validate_plan_target(
    plan: &ObjectMethodUseCasePlan,
    target: ObjectMethodUseCaseTarget,
) -> Result<(), HalError> {
    if plan.command_plan().object() != target.object_kind() {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL method/object kind mismatch",
        ));
    }
    Ok(())
}

fn plan_aidl_method_call(method: AidlMethodCall) -> Result<ObjectMethodUseCasePlan, HalError> {
    let method_plan = AidlMethodAdapter::plan(method)?;
    Ok(build_plan(
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
    ))
}

fn build_aidl_method_plan_after_live_inner<T, E, F>(
    runtime: &SharedObjectMethodRuntime,
    target: ObjectMethodUseCaseTarget,
    build: F,
) -> Result<(ObjectMethodUseCasePlan, T), ObjectMethodUseCaseBuildError<E>>
where
    F: FnOnce() -> Result<(AidlMethodCall, T), E>,
{
    let mut runtime = runtime.lock().map_err(|_| {
        ObjectMethodUseCaseBuildError::Runtime(HalError::internal(
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
    .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
    let (method, request) = build().map_err(ObjectMethodUseCaseBuildError::Builder)?;
    let plan = plan_aidl_method_call(method).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
    validate_plan_target(&plan, target).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
    plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
        .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
    Ok((plan, request))
}

/// Canonical call-local owner for object-method validation, planning, dispatch,
/// and one-shot execution authority issuance.
///
/// The type is intentionally stateless: all persistent state remains in the
/// object table and the corresponding domain owners.
pub struct ObjectMethodUseCase;

impl ObjectMethodUseCase {
    pub fn execute_query_after_live(
        runtime: &SharedObjectMethodRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        request: ObjectQueryRequest,
    ) -> Result<ObjectQueryResponse, HalError> {
        let target = ObjectMethodUseCaseTarget::new(object_id, generation, object_kind);
        let execution = {
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
            prepare_object_query_request(&query, target, request)?
        };
        finish_object_query_execution(runtime, execution)
    }

    pub fn execute_query_after_live_with_aidl_input_conversion<E, Build>(
        runtime: &SharedObjectMethodRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        method: AidlMethodCall,
        build: Build,
    ) -> Result<ObjectQueryResponse, ObjectMethodUseCaseBuildError<E>>
    where
        Build: FnOnce() -> Result<ObjectQueryRequest, E>,
    {
        let target = ObjectMethodUseCaseTarget::new(object_id, generation, object_kind);
        let execution = {
            let mut runtime = runtime.lock().map_err(|_| {
                ObjectMethodUseCaseBuildError::Runtime(HalError::internal(
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
            .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            let plan = plan_aidl_method_call(method).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            validate_plan_target(&plan, target).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
                .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            let request = build().map_err(ObjectMethodUseCaseBuildError::Builder)?;
            let query = runtime.query();
            prepare_object_query_request(&query, target, request)
                .map_err(ObjectMethodUseCaseBuildError::Runtime)?
        };
        finish_object_query_execution(runtime, execution).map_err(ObjectMethodUseCaseBuildError::Runtime)
    }

    pub fn execute_after_live<T, E, B, Build, Execute>(
        runtime: &SharedObjectMethodRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        build: Build,
        execute: Execute,
    ) -> Result<T, ObjectMethodUseCaseBuildError<E>>
    where
        Build: FnOnce() -> Result<(AidlMethodCall, B), E>,
        Execute: FnOnce(&mut TunerServiceRuntime, ObjectMethodExecutionToken, B) -> Result<T, HalError>,
    {
        let target = ObjectMethodUseCaseTarget::new(object_id, generation, object_kind);
        let mut runtime = runtime.lock().map_err(|_| {
            ObjectMethodUseCaseBuildError::Runtime(HalError::internal(
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
        .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
        let (method, request) = build().map_err(ObjectMethodUseCaseBuildError::Builder)?;
        let plan = plan_aidl_method_call(method).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
        validate_plan_target(&plan, target).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
        plan_object_method_dispatch(&mut runtime, plan.command_plan(), plan.executable_request())
            .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
        ObjectMethodDispatchProof::new(target)
            .consume_for_target(target)
            .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
        execute(
            &mut runtime,
            ObjectMethodExecutionToken::new(target),
            request,
        )
        .map_err(ObjectMethodUseCaseBuildError::Runtime)
    }

    pub fn execute_shared_after_live<T, E, B, Build, Execute>(
        runtime: &SharedObjectMethodRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        build: Build,
        execute: Execute,
    ) -> Result<T, ObjectMethodUseCaseBuildError<E>>
    where
        Build: FnOnce() -> Result<(AidlMethodCall, B), E>,
        Execute:
            FnOnce(SharedObjectMethodRuntime, ObjectMethodExecutionToken, B) -> Result<T, HalError>,
    {
        let target = ObjectMethodUseCaseTarget::new(object_id, generation, object_kind);
        let request = {
            let mut runtime_guard = runtime.lock().map_err(|_| {
                ObjectMethodUseCaseBuildError::Runtime(HalError::internal(
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
            .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            let (method, request) = build().map_err(ObjectMethodUseCaseBuildError::Builder)?;
            let plan = plan_aidl_method_call(method).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            validate_plan_target(&plan, target).map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            plan_object_method_dispatch(
                &mut runtime_guard,
                plan.command_plan(),
                plan.executable_request(),
            )
            .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            ObjectMethodDispatchProof::new(target)
                .consume_for_target(target)
                .map_err(ObjectMethodUseCaseBuildError::Runtime)?;
            request
        };
        execute(
            Arc::clone(runtime),
            ObjectMethodExecutionToken::new(target),
            request,
        )
        .map_err(ObjectMethodUseCaseBuildError::Runtime)
    }

    pub fn preflight_after_live<E, F>(
        runtime: &SharedObjectMethodRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        build: F,
    ) -> Result<AidlApi, ObjectMethodUseCaseBuildError<E>>
    where
        F: FnOnce() -> Result<AidlMethodCall, E>,
    {
        let target = ObjectMethodUseCaseTarget::new(object_id, generation, object_kind);
        let (plan, ()) = build_aidl_method_plan_after_live_inner(runtime, target, || {
            build().map(|method| (method, ()))
        })?;
        Ok(plan.command_plan().api())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_demux::{
        FilterConfig, FilterConfigKind, FilterOpenType, OpenFilterRequest,
    };

    fn snapshot(
        lnb_profile: Option<LnbRegistryProfile>,
        runtime_state: FrontendRuntimeState,
        signal_state: FrontendSignalState,
    ) -> ObjectFrontendStatusSnapshot {
        ObjectFrontendStatusSnapshot {
            backend: FrontendBackendKind::LinuxDvb,
            lnb_profile,
            runtime_state,
            signal_state,
            lnb_voltage: lnb_profile.map(|_| maleicacid_tuner_hal2_lnb::LnbVoltage::None),
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
            Ok(ObjectFrontendStatusValue::DemodLocked(true))
        );
        assert_eq!(
            object_frontend_status_value(unlocked, ObjectFrontendStatusType::DemodLock),
            Ok(ObjectFrontendStatusValue::DemodLocked(false))
        );
    }

    #[test]
    fn frontend_status_rejects_unsupported_without_shortening_response() {
        let error = object_frontend_status_value(
            snapshot(
                None,
                FrontendRuntimeState::Idle,
                FrontendSignalState::Locked,
            ),
            ObjectFrontendStatusType::Unsupported,
        )
        .expect_err("unsupported frontend status must not be silently dropped");

        assert!(matches!(error, HalError::Unsupported(_)));
    }

    #[test]
    fn av_sync_time_returns_invalid_timestamp_before_first_pcr() {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let demux_entry = {
            let mut guard = runtime.lock().unwrap();
            guard
                .root_open_txn().open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .expect("demux root open succeeds")
        };
        let pcr_open = ObjectMethodUseCase::execute_after_live(
            &runtime,
            demux_entry.object_id(),
            demux_entry.generation(),
            AidlObjectKind::Demux,
            || -> Result<_, HalError> {
                let request = OpenFilterRequest {
                    open_type: FilterOpenType::TsPcr,
                    buffer_size: 4096,
                    callback_present: false,
                };
                Ok((
                    AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                        request.clone(),
                    )),
                    request,
                ))
            },
            |runtime, dispatch, request| {
                runtime.child_open_txn().open_filter_child_runtime_for_demux_object(
                    demux_entry.object_id(),
                    demux_entry.generation(),
                    &request,
                    dispatch,
                )
            },
        )
        .expect("PCR filter child open succeeds");
        {
            let mut guard = runtime.lock().unwrap();
            guard
                .configure_filter_runtime_request(
                    pcr_open.filter_id,
                    FilterConfig {
                        open_type: FilterOpenType::TsPcr,
                        tpid: 0x0100,
                        kind: FilterConfigKind::TsRaw,
                    },
                )
                .expect("PCR filter configure succeeds");
            guard
                .start_filter_runtime(pcr_open.filter_id)
                .expect("PCR filter start succeeds");
        }

        let response = ObjectMethodUseCase::execute_query_after_live(
            &runtime,
            demux_entry.object_id(),
            demux_entry.generation(),
            AidlObjectKind::Demux,
            ObjectQueryRequest::DemuxGetAvSyncTime {
                av_sync_hw_id: pcr_open.filter_id,
            },
        )
        .expect("valid PCR sync id succeeds before first PCR observation");

        assert!(matches!(response, ObjectQueryResponse::AvSyncTime(-1)));
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

    #[test]
    fn frontend_status_reports_committed_lnb_voltage() {
        let mut value = snapshot(
            Some(LnbRegistryProfile::Px4Device15VOnly),
            FrontendRuntimeState::Idle,
            FrontendSignalState::NoSignal,
        );
        value.lnb_voltage = Some(maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V);
        assert_eq!(
            object_frontend_status_value(value, ObjectFrontendStatusType::LnbVoltage),
            Ok(ObjectFrontendStatusValue::LnbVoltage15V)
        );
    }

    #[test]
    fn dvb_rf_lock_uses_carrier_or_demod_lock_snapshot() {
        let carrier = snapshot(
            None,
            FrontendRuntimeState::Tuning { generation: 4 },
            FrontendSignalState::SignalDetected,
        );
        let no_signal = snapshot(
            None,
            FrontendRuntimeState::Tuning { generation: 4 },
            FrontendSignalState::NoSignal,
        );
        assert_eq!(
            object_frontend_status_value(carrier, ObjectFrontendStatusType::RfLock),
            Ok(ObjectFrontendStatusValue::RfLocked(true))
        );
        assert_eq!(
            object_frontend_status_value(no_signal, ObjectFrontendStatusType::RfLock),
            Ok(ObjectFrontendStatusValue::RfLocked(false))
        );
    }

    #[test]
    fn px4_demod_lock_status_is_exposed_from_current_readback_snapshot() {
        let px4 = ObjectFrontendStatusSnapshot {
            backend: FrontendBackendKind::Px4CharDevice,
            lnb_profile: None,
            runtime_state: FrontendRuntimeState::Tuning { generation: 7 },
            signal_state: FrontendSignalState::Locked,
            lnb_voltage: None,
        };

        assert_eq!(
            object_frontend_status_value(px4, ObjectFrontendStatusType::DemodLock),
            Ok(ObjectFrontendStatusValue::DemodLocked(true))
        );
        assert_eq!(
            object_frontend_readiness_value(px4, ObjectFrontendStatusType::DemodLock),
            ObjectFrontendStatusReadinessValue::Stable
        );
    }
}
