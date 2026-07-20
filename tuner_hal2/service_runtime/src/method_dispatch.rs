use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{CommandPlan, RuntimeExecutableRequest};

use crate::boot::TunerServiceRuntime;
use crate::error_mapping::command_dispatch_error_to_hal;
use crate::method_validation::validate_runtime_executable_request;

pub(crate) fn plan_object_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
) -> Result<(), HalError> {
    validate_runtime_executable_request(executable_request.as_ref())?;
    runtime
        .plan_command_dispatch(command_plan, executable_request)
        .map(|_| ())
        .map_err(command_dispatch_error_to_hal)
}
