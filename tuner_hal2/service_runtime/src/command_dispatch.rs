use maleicacid_tuner_hal2_domain_request::{CommandPlan, RuntimeExecutableRequest, RuntimeTransactionName};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

use crate::dispatch::{dispatch_target_for, ServiceRuntimeDispatchTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeCommandDispatchPlan {
    pub command_plan: CommandPlan,
    pub target: ServiceRuntimeDispatchTarget,
    pub executable_request: Option<RuntimeExecutableRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeCommandDispatchError {
    MissingDispatchTarget { transaction: RuntimeTransactionName },
}

impl RuntimeCommandDispatchError {
    pub fn into_hal_error(self) -> HalError {
        match self {
            Self::MissingDispatchTarget { .. } => {
                HalError::internal(HalInternalKind::InvariantViolation, "runtime dispatch target missing")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeCommandDispatcher {
    covered_transaction_count: usize,
}

impl Default for RuntimeCommandDispatcher {
    fn default() -> Self { Self::new() }
}

impl RuntimeCommandDispatcher {
    pub fn new() -> Self {
        Self { covered_transaction_count: crate::dispatch::SERVICE_RUNTIME_DISPATCH_TABLE.len() }
    }

    pub const fn covered_transaction_count(&self) -> usize { self.covered_transaction_count }

    pub fn plan(command_plan: CommandPlan, executable_request: Option<RuntimeExecutableRequest>) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        let Some(target) = dispatch_target_for(command_plan.transaction) else {
            return Err(RuntimeCommandDispatchError::MissingDispatchTarget { transaction: command_plan.transaction });
        };
        Ok(RuntimeCommandDispatchPlan { command_plan, target, executable_request })
    }
}
