use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, HalError,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
    RuntimeExecutableRequest,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;

use crate::boot::OwnerCallbackCleanupArtifactCommand;
use crate::error_mapping::object_table_error_to_hal;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_domain_cleanup::{
    ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor, ObjectDomainCleanupKind,
};
use crate::object_lifecycle::{aidl_object_closeable, AidlObjectCloseability};
use crate::{RuntimeObjectEntry, TunerServiceRuntime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectCloseArtifactCleanupPhase {
    BeforeDomainCleanup,
    AfterDomainCleanup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectCloseArtifactCleanupKind {
    OwnerCallbackRegistration,
    DescendantCallbackRegistration,
    LnbOwnerLossCallbackRegistration,
    DvrStatusNotifier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectCloseArtifactCleanupCommand {
    phase: ObjectCloseArtifactCleanupPhase,
    kind: ObjectCloseArtifactCleanupKind,
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    step: CleanupStep,
}

impl ObjectCloseArtifactCleanupCommand {
    fn new(
        phase: ObjectCloseArtifactCleanupPhase,
        kind: ObjectCloseArtifactCleanupKind,
        entry: &RuntimeObjectEntry,
        step: CleanupStep,
    ) -> Self {
        Self {
            phase,
            kind,
            object_kind: entry.object_kind,
            object_id: entry.object_id,
            generation: entry.generation,
            step,
        }
    }

    fn phase(&self) -> ObjectCloseArtifactCleanupPhase {
        self.phase
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectArtifactCleanupKind {
    OwnerCallbackRegistration,
    DescendantCallbackRegistration,
    LnbOwnerLossCallbackRegistration,
    DvrStatusNotifier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectArtifactCleanupCommand {
    kind: ObjectArtifactCleanupKind,
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    step: CleanupStep,
    owner_callback_cleanup_command: Option<OwnerCallbackCleanupArtifactCommand>,
}

impl ObjectArtifactCleanupCommand {
    fn from_close(command: ObjectCloseArtifactCleanupCommand) -> Self {
        let kind = match command.kind {
            ObjectCloseArtifactCleanupKind::OwnerCallbackRegistration => {
                ObjectArtifactCleanupKind::OwnerCallbackRegistration
            }
            ObjectCloseArtifactCleanupKind::DescendantCallbackRegistration => {
                ObjectArtifactCleanupKind::DescendantCallbackRegistration
            }
            ObjectCloseArtifactCleanupKind::LnbOwnerLossCallbackRegistration => {
                ObjectArtifactCleanupKind::LnbOwnerLossCallbackRegistration
            }
            ObjectCloseArtifactCleanupKind::DvrStatusNotifier => {
                ObjectArtifactCleanupKind::DvrStatusNotifier
            }
        };
        let owner_callback_cleanup_command = owner_callback_cleanup_command_for_parts(
            kind,
            command.object_kind,
            command.object_id,
            command.generation,
        );
        Self {
            kind,
            object_kind: command.object_kind,
            object_id: command.object_id,
            generation: command.generation,
            step: command.step,
            owner_callback_cleanup_command,
        }
    }

    fn new(kind: ObjectArtifactCleanupKind, entry: &RuntimeObjectEntry, step: CleanupStep) -> Self {
        let owner_callback_cleanup_command = owner_callback_cleanup_command_for_parts(
            kind,
            entry.object_kind,
            entry.object_id,
            entry.generation,
        );
        Self {
            kind,
            object_kind: entry.object_kind,
            object_id: entry.object_id,
            generation: entry.generation,
            step,
            owner_callback_cleanup_command,
        }
    }

    pub fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub fn step(&self) -> CleanupStep {
        self.step
    }

    pub fn owner_callback_cleanup_command(&self) -> Option<&OwnerCallbackCleanupArtifactCommand> {
        self.owner_callback_cleanup_command.as_ref()
    }

    pub fn execute_with<E: ObjectArtifactCleanupExecutor>(
        self,
        executor: &mut E,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        match self.kind {
            ObjectArtifactCleanupKind::OwnerCallbackRegistration => {
                executor.execute_owner_callback_cleanup(self)
            }
            ObjectArtifactCleanupKind::DescendantCallbackRegistration => {
                executor.execute_descendant_callback_cleanup(self)
            }
            ObjectArtifactCleanupKind::LnbOwnerLossCallbackRegistration => {
                executor.execute_lnb_owner_loss_callback_cleanup(self)
            }
            ObjectArtifactCleanupKind::DvrStatusNotifier => {
                executor.execute_dvr_status_notifier_cleanup(self)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectRuntimeCleanupKind {
    ClosePublicRuntimeUnregister,
    DropLeakPublicRuntimeUnregister,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRuntimeCleanupCommand {
    kind: ObjectRuntimeCleanupKind,
    entries: Vec<RuntimeObjectEntry>,
}

impl ObjectRuntimeCleanupCommand {
    fn new(kind: ObjectRuntimeCleanupKind, entries: Vec<RuntimeObjectEntry>) -> Self {
        Self { kind, entries }
    }

    pub fn execute(
        &self,
        runtime: &mut TunerServiceRuntime,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        match self.kind {
            ObjectRuntimeCleanupKind::ClosePublicRuntimeUnregister => {
                unregister_public_runtime_entries_for_close(runtime, &self.entries)
            }
            ObjectRuntimeCleanupKind::DropLeakPublicRuntimeUnregister => {
                unregister_public_runtime_entries_for_drop_leak(runtime, &self.entries)
            }
        }
    }
}

pub trait ObjectCloseRuntimeExecutor {
    fn execute_runtime_cleanup(
        &mut self,
        command: ObjectRuntimeCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure>;
}

pub trait ObjectArtifactCleanupExecutor {
    fn execute_owner_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure>;

    fn execute_descendant_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure>;

    fn execute_lnb_owner_loss_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure>;

    fn execute_dvr_status_notifier_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure>;
}

pub type ObjectCloseUseCaseResult = Result<(), ObjectCloseCleanupFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectCloseUseCasePlan {
    target: RuntimeObjectEntry,
    cascade_entries: Vec<RuntimeObjectEntry>,
    domain_cleanup_step: CleanupStep,
    artifact_cleanup_commands: Vec<ObjectCloseArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

impl ObjectCloseUseCasePlan {
    pub fn target(&self) -> &RuntimeObjectEntry {
        &self.target
    }

    pub fn execute_cleanup_with_executor<R, D, A>(
        self,
        runtime_executor: &mut R,
        domain_executor: &mut D,
        artifact_executor: &mut A,
    ) -> ObjectCloseUseCaseResult
    where
        R: ObjectCloseRuntimeExecutor,
        D: ObjectDomainCleanupExecutor,
        A: ObjectArtifactCleanupExecutor,
    {
        let mut cleanup_collector = FirstErrorCollector::new();
        for command in self
            .artifact_cleanup_commands
            .iter()
            .copied()
            .filter(|command| {
                command.phase() == ObjectCloseArtifactCleanupPhase::BeforeDomainCleanup
            })
        {
            cleanup_collector.push_result(
                ObjectArtifactCleanupCommand::from_close(command).execute_with(artifact_executor),
            );
        }
        for command in self.domain_cleanup_commands.iter().copied() {
            let outcome = command.execute_with(domain_executor);
            cleanup_collector.push_result(
                outcome.result().map_err(|error| {
                    ObjectCloseCleanupFailure::new(self.domain_cleanup_step, error)
                }),
            );
        }
        for command in self
            .artifact_cleanup_commands
            .iter()
            .copied()
            .filter(|command| {
                command.phase() == ObjectCloseArtifactCleanupPhase::AfterDomainCleanup
            })
        {
            cleanup_collector.push_result(
                ObjectArtifactCleanupCommand::from_close(command).execute_with(artifact_executor),
            );
        }
        cleanup_collector.push_result(runtime_executor.execute_runtime_cleanup(
            ObjectRuntimeCleanupCommand::new(
                ObjectRuntimeCleanupKind::ClosePublicRuntimeUnregister,
                self.cascade_entries.clone(),
            ),
        ));
        cleanup_collector.into_result()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectDropLeakQuarantinePlan {
    cascade_entries: Vec<RuntimeObjectEntry>,
    artifact_cleanup_commands: Vec<ObjectArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

impl ObjectDropLeakQuarantinePlan {
    pub fn execute_terminalization_with_executor<R, D, A>(
        &self,
        runtime_executor: &mut R,
        domain_executor: &mut D,
        artifact_executor: &mut A,
    ) -> Result<(), ObjectCloseCleanupFailure>
    where
        R: ObjectCloseRuntimeExecutor,
        D: ObjectDomainCleanupExecutor,
        A: ObjectArtifactCleanupExecutor,
    {
        let mut cleanup_collector = FirstErrorCollector::new();
        for command in self.domain_cleanup_commands.iter().copied() {
            let outcome = command.execute_with(domain_executor);
            cleanup_collector.push_result(outcome.result().map_err(|error| {
                ObjectCloseCleanupFailure::new(CleanupStep::ReleaseBackend, error)
            }));
        }
        for command in self.artifact_cleanup_commands.iter().copied() {
            cleanup_collector.push_result(command.execute_with(artifact_executor));
        }
        cleanup_collector.push_result(runtime_executor.execute_runtime_cleanup(
            ObjectRuntimeCleanupCommand::new(
                ObjectRuntimeCleanupKind::DropLeakPublicRuntimeUnregister,
                self.cascade_entries.clone(),
            ),
        ));
        cleanup_collector.into_result()
    }
}

#[derive(Clone, Debug)]
pub struct ObjectCloseCleanupFailure {
    step: CleanupStep,
    error: HalError,
}

impl ObjectCloseCleanupFailure {
    pub fn new(step: CleanupStep, error: HalError) -> Self {
        Self { step, error }
    }

    pub fn step(&self) -> CleanupStep {
        self.step
    }

    pub fn error(&self) -> &HalError {
        &self.error
    }

    pub fn into_error(self) -> HalError {
        self.error
    }
}

fn entry_requires_public_runtime_unregister(entry: &RuntimeObjectEntry) -> bool {
    matches!(
        entry.object_kind,
        AidlObjectKind::Demux
            | AidlObjectKind::Filter
            | AidlObjectKind::Dvr
            | AidlObjectKind::Descrambler
    )
}

fn callback_registration_api_for_close_parts(object_kind: AidlObjectKind) -> Option<AidlApi> {
    match object_kind {
        AidlObjectKind::Frontend => Some(AidlApi::FrontendSetCallback),
        AidlObjectKind::Filter => Some(AidlApi::DemuxOpenFilter),
        AidlObjectKind::Dvr => Some(AidlApi::DemuxOpenDvr),
        AidlObjectKind::Lnb => Some(AidlApi::LnbSetCallback),
        _ => None,
    }
}

fn callback_registration_api_for_close_entry(entry: &RuntimeObjectEntry) -> Option<AidlApi> {
    callback_registration_api_for_close_parts(entry.object_kind)
}

fn entry_has_callback_registration(
    runtime: &TunerServiceRuntime,
    entry: &RuntimeObjectEntry,
) -> bool {
    callback_registration_api_for_close_entry(entry).is_some_and(|api| {
        runtime.has_callback_registration(entry.object_kind, entry.object_id, entry.generation, api)
    })
}

fn callback_cleanup_failure_message_for_kind(kind: ObjectArtifactCleanupKind) -> &'static str {
    match kind {
        ObjectArtifactCleanupKind::OwnerCallbackRegistration => {
            "callback store cleanup failed during owner callback cleanup"
        }
        ObjectArtifactCleanupKind::DescendantCallbackRegistration => {
            "callback store cleanup failed during descendant callback cleanup"
        }
        ObjectArtifactCleanupKind::LnbOwnerLossCallbackRegistration => {
            "callback store cleanup failed during LNB owner-loss cleanup"
        }
        ObjectArtifactCleanupKind::DvrStatusNotifier => {
            "DVR status notifier cleanup failed during object cleanup"
        }
    }
}

fn owner_callback_cleanup_command_for_parts(
    kind: ObjectArtifactCleanupKind,
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Option<OwnerCallbackCleanupArtifactCommand> {
    match kind {
        ObjectArtifactCleanupKind::OwnerCallbackRegistration
        | ObjectArtifactCleanupKind::DescendantCallbackRegistration
        | ObjectArtifactCleanupKind::LnbOwnerLossCallbackRegistration => {
            Some(OwnerCallbackCleanupArtifactCommand::new(
                object_kind,
                object_id,
                generation,
                callback_registration_api_for_close_parts(object_kind),
                callback_cleanup_failure_message_for_kind(kind),
            ))
        }
        ObjectArtifactCleanupKind::DvrStatusNotifier => None,
    }
}

fn artifact_cleanup_commands_for_close_plan(
    runtime: &TunerServiceRuntime,
    target: &RuntimeObjectEntry,
    entries: &[RuntimeObjectEntry],
) -> Vec<ObjectCloseArtifactCleanupCommand> {
    let mut commands = Vec::new();
    if entry_has_callback_registration(runtime, target) {
        commands.push(ObjectCloseArtifactCleanupCommand::new(
            ObjectCloseArtifactCleanupPhase::BeforeDomainCleanup,
            ObjectCloseArtifactCleanupKind::OwnerCallbackRegistration,
            target,
            CleanupStep::UnregisterRuntime,
        ));
    }

    for entry in entries {
        if entry.object_id == target.object_id && entry.generation == target.generation {
            continue;
        }
        if entry_has_callback_registration(runtime, entry) {
            let cleanup_kind = if target.object_kind == AidlObjectKind::Frontend
                && entry.object_kind == AidlObjectKind::Lnb
            {
                ObjectCloseArtifactCleanupKind::LnbOwnerLossCallbackRegistration
            } else {
                ObjectCloseArtifactCleanupKind::DescendantCallbackRegistration
            };
            commands.push(ObjectCloseArtifactCleanupCommand::new(
                ObjectCloseArtifactCleanupPhase::AfterDomainCleanup,
                cleanup_kind,
                entry,
                CleanupStep::UnregisterRuntime,
            ));
        }
        if entry.object_kind == AidlObjectKind::Dvr {
            commands.push(ObjectCloseArtifactCleanupCommand::new(
                ObjectCloseArtifactCleanupPhase::AfterDomainCleanup,
                ObjectCloseArtifactCleanupKind::DvrStatusNotifier,
                entry,
                CleanupStep::StopWorker,
            ));
        }
    }
    commands
}

fn close_domain_cleanup_command_for_entry(
    entry: &RuntimeObjectEntry,
) -> Option<ObjectDomainCleanupCommand> {
    ObjectDomainCleanupKind::close_for_object_kind(entry.object_kind).map(|cleanup_kind| {
        ObjectDomainCleanupCommand::new(
            entry.object_kind,
            entry.object_id,
            entry.generation,
            cleanup_kind,
        )
    })
}

fn drop_leak_domain_cleanup_command_for_entry(
    entry: &RuntimeObjectEntry,
) -> Option<ObjectDomainCleanupCommand> {
    ObjectDomainCleanupKind::drop_leak_for_object_kind(entry.object_kind).map(|cleanup_kind| {
        ObjectDomainCleanupCommand::new(
            entry.object_kind,
            entry.object_id,
            entry.generation,
            cleanup_kind,
        )
    })
}

fn unregister_public_runtime_entries_for_close(
    runtime: &mut TunerServiceRuntime,
    entries: &[RuntimeObjectEntry],
) -> Result<(), ObjectCloseCleanupFailure> {
    let public_runtime_entries = entries
        .iter()
        .filter(|entry| entry_requires_public_runtime_unregister(entry))
        .collect::<Vec<_>>();
    let mut preflight_collector = FirstErrorCollector::new();
    for entry in &public_runtime_entries {
        preflight_collector
            .push_result(runtime.validate_public_runtime_for_closed_aidl_entry(entry));
    }
    preflight_collector
        .into_result()
        .map_err(|error| ObjectCloseCleanupFailure::new(CleanupStep::UnregisterRuntime, error))?;

    let mut cleanup_collector = FirstErrorCollector::new();
    for entry in public_runtime_entries {
        cleanup_collector
            .push_result(runtime.unregister_public_runtime_for_closed_aidl_entry(entry));
    }
    cleanup_collector
        .into_result()
        .map_err(|error| ObjectCloseCleanupFailure::new(CleanupStep::UnregisterRuntime, error))
}

fn unregister_public_runtime_entries_for_drop_leak(
    runtime: &mut TunerServiceRuntime,
    entries: &[RuntimeObjectEntry],
) -> Result<(), ObjectCloseCleanupFailure> {
    let public_runtime_entries = entries
        .iter()
        .filter(|entry| entry_requires_public_runtime_unregister(entry))
        .collect::<Vec<_>>();

    let mut preflight_collector = FirstErrorCollector::new();
    for entry in &public_runtime_entries {
        preflight_collector
            .push_result(runtime.validate_public_runtime_for_drop_leak_aidl_entry(entry));
    }
    preflight_collector
        .into_result()
        .map_err(|error| ObjectCloseCleanupFailure::new(CleanupStep::UnregisterRuntime, error))?;

    let mut cleanup_collector = FirstErrorCollector::new();
    for entry in public_runtime_entries {
        cleanup_collector
            .push_result(runtime.unregister_public_runtime_for_drop_leak_aidl_entry(entry));
    }
    cleanup_collector
        .into_result()
        .map_err(|error| ObjectCloseCleanupFailure::new(CleanupStep::UnregisterRuntime, error))
}

pub fn close_object_use_case(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    method: AidlMethodCall,
) -> Result<ObjectCloseUseCasePlan, HalError> {
    match plan_and_begin_object_close_method_call_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        method,
        CleanupStep::ReleaseBackend,
    )? {
        AidlObjectCloseability::BeginClose => {}
    }

    let cascade_entries = match object_close_cascade_entries(runtime, object_id, generation) {
        Ok(entries) => entries,
        Err(cleanup_error) => {
            return match mark_object_close_cleanup_failed_cascade(
                runtime,
                object_id,
                generation,
                CleanupStep::ReleaseLedger,
                "object close cascade entries could not be collected",
            ) {
                Ok(()) => Err(cleanup_error),
                Err(mark_error) => Err(compose_primary_cleanup_failure(
                    "object close cascade entry collection failed and cleanup-failed marking failed",
                    cleanup_error,
                    mark_error,
                )),
            };
        }
    };
    let target = match cascade_entries
        .iter()
        .find(|entry| entry.object_id == object_id && entry.generation == generation)
        .cloned()
    {
        Some(target) => target,
        None => {
            let cleanup_error = HalError::cleanup_failed(
                "object close cascade plan",
                "target object was not present in close cascade entries",
            );
            return match mark_object_close_cleanup_failed_cascade(
                runtime,
                object_id,
                generation,
                CleanupStep::ReleaseLedger,
                "object close cascade plan target missing",
            ) {
                Ok(()) => Err(cleanup_error),
                Err(mark_error) => Err(compose_primary_cleanup_failure(
                    "object close cascade target resolution failed and cleanup-failed marking failed",
                    cleanup_error,
                    mark_error,
                )),
            };
        }
    };
    let artifact_cleanup_commands =
        artifact_cleanup_commands_for_close_plan(runtime, &target, &cascade_entries);
    let domain_cleanup_commands = cascade_entries
        .iter()
        .filter_map(close_domain_cleanup_command_for_entry)
        .collect();

    Ok(ObjectCloseUseCasePlan {
        target,
        cascade_entries,
        domain_cleanup_step: CleanupStep::ReleaseBackend,
        artifact_cleanup_commands,
        domain_cleanup_commands,
    })
}

pub fn finish_object_close_use_case(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    cleanup_result: Result<(), ObjectCloseCleanupFailure>,
) -> Result<(), HalError> {
    if let Err(cleanup_failure) = cleanup_result {
        match mark_object_close_cleanup_failed_cascade(
            runtime,
            object_id,
            generation,
            cleanup_failure.step(),
            "object close cleanup failure could not be recorded",
        ) {
            Ok(()) => return Err(cleanup_failure.into_error()),
            Err(mark_error) => {
                return Err(compose_primary_cleanup_failure(
                    "object close cleanup failed and cleanup-failed marking failed",
                    cleanup_failure.into_error(),
                    mark_error,
                ));
            }
        }
    }

    if let Err(cleanup_error) = object_close_cascade_entries(runtime, object_id, generation) {
        return match mark_object_close_cleanup_failed_cascade(
            runtime,
            object_id,
            generation,
            CleanupStep::ReleaseLedger,
            "object close finalization failure could not be recorded",
        ) {
            Ok(()) => Err(cleanup_error),
            Err(mark_error) => Err(compose_primary_cleanup_failure(
                "object close finalization failed and cleanup-failed marking failed",
                cleanup_error,
                mark_error,
            )),
        };
    }

    if let Err(cleanup_error) =
        commit_object_close_cascade(runtime, object_id, generation).map(|_| ())
    {
        return match mark_object_close_cleanup_failed_cascade(
            runtime,
            object_id,
            generation,
            CleanupStep::ReleaseLedger,
            "object close commit failure could not be recorded",
        ) {
            Ok(()) => Err(cleanup_error),
            Err(mark_error) => Err(compose_primary_cleanup_failure(
                "object close commit failed and cleanup-failed marking failed",
                cleanup_error,
                mark_error,
            )),
        };
    }

    Ok(())
}

pub(crate) fn plan_object_close_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
) -> Result<AidlObjectCloseability, HalError> {
    let closeability = aidl_object_closeable(runtime, object_id, generation, object_kind)?;
    plan_object_method_dispatch(runtime, command_plan, executable_request).map(|_| closeability)
}

pub(crate) fn plan_and_begin_object_close_method_call_dispatch(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    method: AidlMethodCall,
    step: CleanupStep,
) -> Result<AidlObjectCloseability, HalError> {
    let method_plan = AidlMethodAdapter::plan(method)?;
    plan_and_begin_object_close_command_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
        step,
    )
}

pub(crate) fn plan_and_begin_object_close_command_dispatch(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
    step: CleanupStep,
) -> Result<AidlObjectCloseability, HalError> {
    match plan_object_close_method_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        command_plan,
        executable_request,
    )? {
        AidlObjectCloseability::BeginClose => {
            begin_object_close_cascade(runtime, object_id, generation, step)?;
            Ok(AidlObjectCloseability::BeginClose)
        }
    }
}

pub(crate) fn begin_object_close_cascade(
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

pub(crate) fn mark_object_close_cleanup_failed_cascade(
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

pub(crate) fn object_close_cascade_entries(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Vec<RuntimeObjectEntry>, HalError> {
    runtime
        .object_table()
        .close_cascade_entries(object_id, generation)
        .map_err(object_table_error_to_hal)
}

pub(crate) fn commit_object_close_cascade(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Vec<RuntimeObjectEntry>, HalError> {
    runtime
        .object_table_mut()
        .commit_close_cascade(object_id, generation)
        .map_err(object_table_error_to_hal)
}

fn artifact_cleanup_commands_for_drop_leak_plan(
    runtime: &TunerServiceRuntime,
    entries: &[RuntimeObjectEntry],
) -> Vec<ObjectArtifactCleanupCommand> {
    let mut commands = Vec::new();
    for entry in entries {
        if entry_has_callback_registration(runtime, entry) {
            commands.push(ObjectArtifactCleanupCommand::new(
                ObjectArtifactCleanupKind::OwnerCallbackRegistration,
                entry,
                CleanupStep::UnregisterRuntime,
            ));
        }
        if entry.object_kind == AidlObjectKind::Dvr {
            commands.push(ObjectArtifactCleanupCommand::new(
                ObjectArtifactCleanupKind::DvrStatusNotifier,
                entry,
                CleanupStep::StopWorker,
            ));
        }
    }
    commands
}

pub fn quarantine_object_drop_leak_use_case(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<ObjectDropLeakQuarantinePlan, HalError> {
    let cascade_entries = quarantine_object_cascade(runtime, object_id, generation)?;
    let artifact_cleanup_commands =
        artifact_cleanup_commands_for_drop_leak_plan(runtime, &cascade_entries);
    let domain_cleanup_commands = cascade_entries
        .iter()
        .filter_map(drop_leak_domain_cleanup_command_for_entry)
        .collect();
    Ok(ObjectDropLeakQuarantinePlan {
        cascade_entries,
        artifact_cleanup_commands,
        domain_cleanup_commands,
    })
}

pub(crate) fn quarantine_object_cascade(
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
    use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlObjectKind, CommandPlan};
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

    #[test]
    fn finish_close_use_case_commits_after_successful_cleanup_report() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(4),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(4),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        begin_object_close_cascade(
            &mut runtime,
            AidlObjectId(4),
            AidlObjectGeneration(1),
            CleanupStep::ReleaseBackend,
        )
        .expect("begin close succeeds");

        finish_object_close_use_case(
            &mut runtime,
            AidlObjectId(4),
            AidlObjectGeneration(1),
            Ok(()),
        )
        .expect("finish succeeds");

        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(4))
                .expect("object remains tracked")
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closed
        );
    }

    #[test]
    fn finish_close_use_case_marks_cleanup_failed_after_cleanup_failure() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(5),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(5),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        begin_object_close_cascade(
            &mut runtime,
            AidlObjectId(5),
            AidlObjectGeneration(1),
            CleanupStep::ReleaseBackend,
        )
        .expect("begin close succeeds");

        let result = finish_object_close_use_case(
            &mut runtime,
            AidlObjectId(5),
            AidlObjectGeneration(1),
            Err(ObjectCloseCleanupFailure::new(
                CleanupStep::ReleaseBackend,
                HalError::cleanup_failed(
                    "object close domain cleanup test",
                    "domain cleanup failed for close use-case test",
                ),
            )),
        );

        assert!(result.is_err());
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(5))
                .expect("object remains tracked")
                .lifecycle,
            crate::RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::ReleaseBackend
            }
        );
    }

    #[test]
    fn plan_and_begin_close_rejects_closed_object() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(3),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(3),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        runtime
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(3),
                AidlObjectGeneration(1),
                CleanupStep::ReleaseBackend,
            )
            .expect("begin close succeeds");
        runtime
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(3), AidlObjectGeneration(1))
            .expect("commit close succeeds");

        assert!(plan_and_begin_object_close_command_dispatch(
            &mut runtime,
            AidlObjectId(3),
            AidlObjectGeneration(1),
            AidlObjectKind::Demux,
            CommandPlan::for_api(AidlObjectKind::Demux, AidlApi::DemuxClose)
                .expect("close command plan exists"),
            None,
            CleanupStep::StopWorker,
        )
        .is_err());
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(3))
                .expect("object remains tracked")
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closed
        );
    }
}
