use maleicacid_tuner_hal2_common::{HalError, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, RuntimeExecutableRequest,
};

use crate::boot::TunerServiceRuntime;
use crate::error_mapping::command_dispatch_error_to_hal;
use crate::method_validation::validate_runtime_executable_request;
use crate::object_lifecycle::aidl_object_live;

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

impl TunerServiceRuntime {
    pub fn plan_object_method_dispatch_for_object(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        command_plan: CommandPlan,
        executable_request: Option<RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        if command_plan.object() != object_kind {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL method/object kind mismatch",
            ));
        }
        aidl_object_live(self, object_id, generation, object_kind)?;
        plan_object_method_dispatch(self, command_plan, executable_request)
    }
}
