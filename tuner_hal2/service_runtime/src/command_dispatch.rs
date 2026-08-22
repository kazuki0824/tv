use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::{
    CommandPlan, RuntimeExecutableRequest, RuntimeTransactionName,
};

use crate::dispatch::{dispatch_target_for, ServiceRuntimeDispatchTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeCommandDispatchPlan {
    pub command_plan: CommandPlan,
    pub target: ServiceRuntimeDispatchTarget,
    pub executable_request: Option<RuntimeExecutableRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeCommandDispatchError {
    MissingCommandPlan,
    MissingDispatchTarget { transaction: RuntimeTransactionName },
    RuntimeLockPoison { transaction: RuntimeTransactionName },
    ServiceCritical,
}

impl RuntimeCommandDispatchError {
    pub fn into_hal_error(self) -> HalError {
        match self {
            Self::MissingCommandPlan => HalError::internal(
                HalInternalKind::InvariantViolation,
                "AIDL command plan is missing from the transaction table",
            ),
            Self::MissingDispatchTarget { .. } => HalError::internal(
                HalInternalKind::InvariantViolation,
                "runtime dispatch target missing",
            ),
            Self::RuntimeLockPoison { .. } => HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while planning method dispatch",
            ),
            Self::ServiceCritical => HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime is terminal after an unfenced cleanup failure",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeCommandDispatcher;

impl RuntimeCommandDispatcher {
    pub const fn new() -> Self {
        Self
    }

    pub fn plan(
        command_plan: CommandPlan,
        executable_request: Option<RuntimeExecutableRequest>,
    ) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        let Some(target) = dispatch_target_for(command_plan.transaction()) else {
            return Err(RuntimeCommandDispatchError::MissingDispatchTarget {
                transaction: command_plan.transaction(),
            });
        };
        Ok(RuntimeCommandDispatchPlan {
            command_plan,
            target,
            executable_request,
        })
    }
}
