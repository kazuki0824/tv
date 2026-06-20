use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, RuntimeExecutableRequest,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;

use crate::error_mapping::object_table_error_to_hal;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_lifecycle::aidl_object_closeable;
use crate::{RuntimeObjectEntry, TunerServiceRuntime};

pub fn plan_object_close_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
) -> Result<(), HalError> {
    aidl_object_closeable(runtime, object_id, generation, object_kind)?;
    plan_object_method_dispatch(runtime, command_plan, executable_request)
}

pub fn plan_and_begin_object_close_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
    step: CleanupStep,
) -> Result<(), HalError> {
    plan_object_close_method_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        command_plan,
        executable_request,
    )?;
    begin_object_close_cascade(runtime, object_id, generation, step)
}

pub fn begin_object_close_cascade(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    step: CleanupStep,
) -> Result<(), HalError> {
    runtime
        .object_table_mut()
        .begin_close_cascade(object_id, generation, step)
        .map(|_| ())
        .map_err(object_table_error_to_hal)
}

pub fn mark_object_close_cleanup_failed_cascade(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    step: CleanupStep,
    detail: &'static str,
) -> Result<(), HalError> {
    runtime
        .object_table_mut()
        .mark_cleanup_failed_cascade(object_id, generation, step)
        .map(|_| ())
        .map_err(|error| {
            let mapped = object_table_error_to_hal(error);
            compose_primary_cleanup_failure(
                detail,
                HalError::cleanup_failed("object close cleanup failed marking", detail),
                mapped,
            )
        })
}

pub fn commit_object_close_cascade(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Vec<RuntimeObjectEntry>, HalError> {
    runtime
        .object_table_mut()
        .commit_close_cascade(object_id, generation)
        .map_err(object_table_error_to_hal)
}

pub fn quarantine_object_cascade(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Vec<RuntimeObjectEntry>, HalError> {
    runtime
        .object_table_mut()
        .quarantine_cascade(object_id, generation)
        .map_err(object_table_error_to_hal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeObjectEntry, RuntimeOwnerRelation};
    use maleicacid_tuner_hal2_domain_request::AidlObjectKind;
    use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

    #[test]
    fn begin_close_cascade_moves_live_object_to_closing() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(1),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(1),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");

        begin_object_close_cascade(
            &mut runtime,
            AidlObjectId(1),
            AidlObjectGeneration(1),
            CleanupStep::StopWorker,
        )
        .expect("begin close succeeds");
    }

    #[test]
    fn begin_close_cascade_rejects_second_begin_for_same_target_object() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(2),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(2),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");

        begin_object_close_cascade(
            &mut runtime,
            AidlObjectId(2),
            AidlObjectGeneration(1),
            CleanupStep::StopWorker,
        )
        .expect("first begin close succeeds");

        assert!(begin_object_close_cascade(
            &mut runtime,
            AidlObjectId(2),
            AidlObjectGeneration(1),
            CleanupStep::UnregisterRuntime,
        )
        .is_err());
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(2))
                .expect("object remains tracked")
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closing {
                step: CleanupStep::StopWorker
            }
        );
    }
}
