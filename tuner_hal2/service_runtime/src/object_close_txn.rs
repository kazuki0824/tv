use std::collections::BTreeMap;

use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, HalError, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
    RuntimeExecutableRequest,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;

use crate::boot::OwnerCallbackCleanupArtifactCommand;
use crate::cleanup_execution::{
    CleanupExecutionDiagnosticSnapshot, CleanupExecutionReport, CleanupExecutionStepOutcome,
    SharedCleanupDiagnostics,
};
use crate::error_mapping::object_table_error_to_hal;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_domain_cleanup::{
    ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor, ObjectDomainCleanupKind,
    ObjectDomainCleanupOutcome,
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

#[derive(Debug, Eq, PartialEq)]
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
pub enum ObjectArtifactCleanupKind {
    OwnerCallbackRegistration,
    DescendantCallbackRegistration,
    LnbOwnerLossCallbackRegistration,
    DvrStatusNotifier,
}

#[derive(Debug, Eq, PartialEq)]
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

    pub fn kind(&self) -> ObjectArtifactCleanupKind {
        self.kind
    }

    pub fn owner_callback_cleanup_command(&self) -> Option<&OwnerCallbackCleanupArtifactCommand> {
        self.owner_callback_cleanup_command.as_ref()
    }

    pub fn execute_outcome_with<E: ObjectArtifactCleanupExecutor>(
        self,
        executor: &mut E,
    ) -> ObjectCleanupStepOutcome {
        let kind = self.kind;
        let object_kind = self.object_kind;
        let object_id = self.object_id;
        let generation = self.generation;
        let step = self.step;
        let result = match kind {
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
        };
        ObjectCleanupStepOutcome::artifact(kind, object_kind, object_id, generation, step, result)
    }

    pub fn execute_with<E: ObjectArtifactCleanupExecutor>(
        self,
        executor: &mut E,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        self.execute_outcome_with(executor).into_result()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectRuntimeCleanupKind {
    ClosePublicRuntimeUnregister,
    DropLeakPublicRuntimeUnregister,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectRuntimeCleanupCommand {
    kind: ObjectRuntimeCleanupKind,
    entries: Vec<RuntimeObjectEntry>,
}

impl ObjectRuntimeCleanupCommand {
    fn new(kind: ObjectRuntimeCleanupKind, entries: Vec<RuntimeObjectEntry>) -> Self {
        Self { kind, entries }
    }

    pub fn kind(&self) -> ObjectRuntimeCleanupKind {
        self.kind
    }

    pub fn entries(&self) -> &[RuntimeObjectEntry] {
        &self.entries
    }

    pub fn execute(
        self,
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

    pub fn execute_outcome_with<E: ObjectCloseRuntimeExecutor>(
        self,
        executor: &mut E,
    ) -> ObjectCleanupStepOutcome {
        let kind = self.kind;
        let entries = self.entries.clone();
        let result = executor.execute_runtime_cleanup(self);
        ObjectCleanupStepOutcome::runtime(kind, entries, result)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectCleanupExecutionKind {
    Artifact(ObjectArtifactCleanupKind),
    Domain(ObjectDomainCleanupKind),
    Runtime(ObjectRuntimeCleanupKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectCleanupObjectTarget {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
}

impl ObjectCleanupObjectTarget {
    const fn new(
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Self {
        Self {
            object_kind,
            object_id,
            generation,
        }
    }

    pub const fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub const fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub const fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }
}

#[derive(Clone, Debug)]
pub enum ObjectCleanupStepOutcome {
    Artifact {
        kind: ObjectArtifactCleanupKind,
        target: ObjectCleanupObjectTarget,
        step: CleanupStep,
        result: Result<(), ObjectCloseCleanupFailure>,
    },
    Domain {
        kind: ObjectDomainCleanupKind,
        target: ObjectCleanupObjectTarget,
        step: CleanupStep,
        result: Result<(), ObjectCloseCleanupFailure>,
    },
    Runtime {
        kind: ObjectRuntimeCleanupKind,
        cascade_entries: Vec<RuntimeObjectEntry>,
        result: Result<(), ObjectCloseCleanupFailure>,
    },
}

impl ObjectCleanupStepOutcome {
    fn artifact(
        kind: ObjectArtifactCleanupKind,
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: CleanupStep,
        result: Result<(), ObjectCloseCleanupFailure>,
    ) -> Self {
        Self::Artifact {
            kind,
            target: ObjectCleanupObjectTarget::new(object_kind, object_id, generation),
            step,
            result,
        }
    }

    fn domain(step: CleanupStep, outcome: ObjectDomainCleanupOutcome) -> Self {
        let result = outcome
            .result()
            .map_err(|error| ObjectCloseCleanupFailure::new(step, error));
        Self::Domain {
            kind: outcome.cleanup_kind(),
            target: ObjectCleanupObjectTarget::new(
                outcome.object_kind(),
                outcome.object_id(),
                outcome.generation(),
            ),
            step,
            result,
        }
    }

    fn runtime(
        kind: ObjectRuntimeCleanupKind,
        cascade_entries: Vec<RuntimeObjectEntry>,
        result: Result<(), ObjectCloseCleanupFailure>,
    ) -> Self {
        Self::Runtime {
            kind,
            cascade_entries,
            result,
        }
    }

    pub const fn execution_kind(&self) -> ObjectCleanupExecutionKind {
        match self {
            Self::Artifact { kind, .. } => ObjectCleanupExecutionKind::Artifact(*kind),
            Self::Domain { kind, .. } => ObjectCleanupExecutionKind::Domain(*kind),
            Self::Runtime { kind, .. } => ObjectCleanupExecutionKind::Runtime(*kind),
        }
    }

    pub fn object_target(&self) -> Option<ObjectCleanupObjectTarget> {
        match self {
            Self::Artifact { target, .. } | Self::Domain { target, .. } => Some(*target),
            Self::Runtime { .. } => None,
        }
    }

    pub fn object_kind(&self) -> Option<AidlObjectKind> {
        match self.object_target() {
            Some(target) => Some(target.object_kind()),
            None => None,
        }
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        match self.object_target() {
            Some(target) => Some(target.object_id()),
            None => None,
        }
    }

    pub fn generation(&self) -> Option<AidlObjectGeneration> {
        match self.object_target() {
            Some(target) => Some(target.generation()),
            None => None,
        }
    }

    pub fn cascade_entries(&self) -> &[RuntimeObjectEntry] {
        match self {
            Self::Runtime {
                cascade_entries, ..
            } => cascade_entries,
            Self::Artifact { .. } | Self::Domain { .. } => &[],
        }
    }

    pub fn step(&self) -> CleanupStep {
        match self {
            Self::Artifact { step, .. } | Self::Domain { step, .. } => *step,
            Self::Runtime { .. } => CleanupStep::UnregisterRuntime,
        }
    }

    pub fn result(&self) -> Result<(), ObjectCloseCleanupFailure> {
        match self {
            Self::Artifact { result, .. }
            | Self::Domain { result, .. }
            | Self::Runtime { result, .. } => result.clone(),
        }
    }

    pub fn into_result(self) -> Result<(), ObjectCloseCleanupFailure> {
        match self {
            Self::Artifact { result, .. }
            | Self::Domain { result, .. }
            | Self::Runtime { result, .. } => result,
        }
    }
}

impl CleanupExecutionStepOutcome for ObjectCleanupStepOutcome {
    type Failure = ObjectCloseCleanupFailure;

    fn result(&self) -> Result<(), Self::Failure> {
        ObjectCleanupStepOutcome::result(self)
    }

    fn into_result(self) -> Result<(), Self::Failure> {
        ObjectCleanupStepOutcome::into_result(self)
    }
}

pub type ObjectCleanupExecutionReport =
    CleanupExecutionReport<ObjectCleanupStepOutcome, ObjectCloseCleanupFailure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectCleanupDiagnosticKind {
    Close,
    DropLeakTerminalization,
}

#[derive(Clone, Debug)]
pub struct ObjectCleanupDiagnosticRecord {
    kind: ObjectCleanupDiagnosticKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    report: ObjectCleanupExecutionReport,
    public_error: Option<HalError>,
}

pub type ObjectCleanupDiagnosticSnapshot =
    CleanupExecutionDiagnosticSnapshot<ObjectCleanupDiagnosticRecord>;
pub type SharedObjectCleanupDiagnostics = SharedCleanupDiagnostics<ObjectCleanupDiagnosticRecord>;

impl ObjectCleanupDiagnosticRecord {
    pub fn close(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        report: ObjectCleanupExecutionReport,
        public_error: Option<HalError>,
    ) -> Self {
        Self {
            kind: ObjectCleanupDiagnosticKind::Close,
            object_id,
            generation,
            report,
            public_error,
        }
    }

    pub fn drop_leak_terminalization(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        report: ObjectCleanupExecutionReport,
        public_error: Option<HalError>,
    ) -> Self {
        Self {
            kind: ObjectCleanupDiagnosticKind::DropLeakTerminalization,
            object_id,
            generation,
            report,
            public_error,
        }
    }

    pub fn kind(&self) -> ObjectCleanupDiagnosticKind {
        self.kind
    }

    pub fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub fn report(&self) -> &ObjectCleanupExecutionReport {
        &self.report
    }

    pub fn public_error(&self) -> Option<&HalError> {
        self.public_error.as_ref()
    }
}

#[must_use = "dropping this value leaves the cleanup obligation pending and reissuable"]
#[derive(Debug, Eq, PartialEq)]
pub struct CloseCleanupAuthority {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    key: CloseCleanupAuthorityKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CloseCleanupObligation {
    generation: AidlObjectGeneration,
    obligation_id: u64,
    active_attempt: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CloseCleanupAuthorityKey {
    pub(crate) obligation_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CloseCleanupAttemptKey {
    pub(crate) obligation_id: u64,
    pub(crate) attempt_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseCleanupAttemptOutcome {
    Complete,
    Pending { step: CleanupStep },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectCloseTxnStateError {
    InvalidAuthority,
    IdentifierExhausted,
}

#[must_use = "a started cleanup attempt must be finished against its generation fence"]
#[derive(Debug, Eq, PartialEq)]
pub struct CloseCleanupAttemptCompletion {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    key: CloseCleanupAttemptKey,
}

#[must_use = "the cleanup attempt owns the only authority to perform these side effects"]
#[derive(Debug, Eq, PartialEq)]
pub struct ObjectCloseCleanupAttempt {
    completion: CloseCleanupAttemptCompletion,
    cascade_entries: Vec<RuntimeObjectEntry>,
    domain_cleanup_step: CleanupStep,
    artifact_cleanup_commands: Vec<ObjectCloseArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

#[must_use = "begin_cleanup_attempt must be called immediately before external cleanup"]
#[derive(Debug, Eq, PartialEq)]
pub struct ObjectCloseUseCasePlan {
    authority: CloseCleanupAuthority,
    cascade_entries: Vec<RuntimeObjectEntry>,
    domain_cleanup_step: CleanupStep,
    artifact_cleanup_commands: Vec<ObjectCloseArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

impl ObjectCloseUseCasePlan {
    pub fn begin_cleanup_attempt(
        self,
        runtime: &mut TunerServiceRuntime,
    ) -> Result<ObjectCloseCleanupAttempt, HalError> {
        let Self {
            authority,
            cascade_entries,
            domain_cleanup_step,
            artifact_cleanup_commands,
            domain_cleanup_commands,
        } = self;
        let key = runtime
            .object_table_mut()
            .begin_close_cleanup_attempt(
                authority.object_id,
                authority.generation,
                authority.key,
            )
            .map_err(object_table_error_to_hal)?;
        Ok(ObjectCloseCleanupAttempt {
            completion: CloseCleanupAttemptCompletion {
                object_id: authority.object_id,
                generation: authority.generation,
                key,
            },
            cascade_entries,
            domain_cleanup_step,
            artifact_cleanup_commands,
            domain_cleanup_commands,
        })
    }
}

impl ObjectCloseCleanupAttempt {
    pub fn execute_cleanup_report_with_executor<R, D, A>(
        self,
        runtime_executor: &mut R,
        domain_executor: &mut D,
        artifact_executor: &mut A,
    ) -> (CloseCleanupAttemptCompletion, ObjectCleanupExecutionReport)
    where
        R: ObjectCloseRuntimeExecutor,
        D: ObjectDomainCleanupExecutor,
        A: ObjectArtifactCleanupExecutor,
    {
        let Self {
            completion,
            cascade_entries,
            domain_cleanup_step,
            artifact_cleanup_commands,
            domain_cleanup_commands,
        } = self;
        let mut report = ObjectCleanupExecutionReport::new();
        let mut before_domain = Vec::new();
        let mut after_domain = Vec::new();
        for command in artifact_cleanup_commands {
            match command.phase() {
                ObjectCloseArtifactCleanupPhase::BeforeDomainCleanup => before_domain.push(command),
                ObjectCloseArtifactCleanupPhase::AfterDomainCleanup => after_domain.push(command),
            }
        }
        for command in before_domain {
            report.push(
                ObjectArtifactCleanupCommand::from_close(command)
                    .execute_outcome_with(artifact_executor),
            );
        }
        for command in domain_cleanup_commands {
            let outcome = command.execute_with(domain_executor);
            report.push(ObjectCleanupStepOutcome::domain(
                domain_cleanup_step,
                outcome,
            ));
        }
        for command in after_domain {
            report.push(
                ObjectArtifactCleanupCommand::from_close(command)
                    .execute_outcome_with(artifact_executor),
            );
        }
        report.push(
            ObjectRuntimeCleanupCommand::new(
                ObjectRuntimeCleanupKind::ClosePublicRuntimeUnregister,
                cascade_entries,
            )
            .execute_outcome_with(runtime_executor),
        );
        (completion, report)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObjectDropLeakQuarantinePlan {
    cascade_entries: Vec<RuntimeObjectEntry>,
    artifact_cleanup_commands: Vec<ObjectArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

impl ObjectDropLeakQuarantinePlan {
    pub fn execute_terminalization_report_with_executor<R, D, A>(
        self,
        runtime_executor: &mut R,
        domain_executor: &mut D,
        artifact_executor: &mut A,
    ) -> ObjectCleanupExecutionReport
    where
        R: ObjectCloseRuntimeExecutor,
        D: ObjectDomainCleanupExecutor,
        A: ObjectArtifactCleanupExecutor,
    {
        let Self {
            cascade_entries,
            artifact_cleanup_commands,
            domain_cleanup_commands,
        } = self;
        let mut report = ObjectCleanupExecutionReport::new();
        for command in domain_cleanup_commands {
            let outcome = command.execute_with(domain_executor);
            report.push(ObjectCleanupStepOutcome::domain(
                CleanupStep::ReleaseBackend,
                outcome,
            ));
        }
        for command in artifact_cleanup_commands {
            report.push(command.execute_outcome_with(artifact_executor));
        }
        report.push(
            ObjectRuntimeCleanupCommand::new(
                ObjectRuntimeCleanupKind::DropLeakPublicRuntimeUnregister,
                cascade_entries,
            )
            .execute_outcome_with(runtime_executor),
        );
        report
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

/// Canonical persistent owner for close-cleanup obligations and attempt
/// fences. `RuntimeObjectTable` owns this value as a private sub-owner and
/// delegates every obligation mutation through these entries.
#[derive(Debug)]
pub struct ObjectCloseTxn {
    obligations: BTreeMap<AidlObjectId, CloseCleanupObligation>,
    next_obligation_id: u64,
    next_attempt_id: u64,
}

impl ObjectCloseTxn {
    pub(crate) fn new() -> Self {
        Self {
            obligations: BTreeMap::new(),
            next_obligation_id: 0,
            next_attempt_id: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.obligations.clear();
        self.next_obligation_id = 0;
        self.next_attempt_id = 0;
    }

    pub(crate) fn clear_obligation(&mut self, object_id: AidlObjectId) {
        self.obligations.remove(&object_id);
    }

    pub(crate) fn issue_cleanup_authority(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<CloseCleanupAuthorityKey, ObjectCloseTxnStateError> {
        if let Some(obligation) = self.obligations.get(&object_id) {
            if obligation.generation != generation || obligation.active_attempt.is_some() {
                return Err(ObjectCloseTxnStateError::InvalidAuthority);
            }
            return Ok(CloseCleanupAuthorityKey {
                obligation_id: obligation.obligation_id,
            });
        }

        let obligation_id = self
            .next_obligation_id
            .checked_add(1)
            .filter(|id| *id != 0)
            .ok_or(ObjectCloseTxnStateError::IdentifierExhausted)?;
        self.next_obligation_id = obligation_id;
        self.obligations.insert(
            object_id,
            CloseCleanupObligation {
                generation,
                obligation_id,
                active_attempt: None,
            },
        );
        Ok(CloseCleanupAuthorityKey { obligation_id })
    }

    pub(crate) fn begin_cleanup_attempt(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        authority: CloseCleanupAuthorityKey,
    ) -> Result<CloseCleanupAttemptKey, ObjectCloseTxnStateError> {
        let obligation = self
            .obligations
            .get(&object_id)
            .copied()
            .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
        if obligation.generation != generation
            || obligation.obligation_id != authority.obligation_id
            || obligation.active_attempt.is_some()
        {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        }
        let attempt_id = self
            .next_attempt_id
            .checked_add(1)
            .filter(|id| *id != 0)
            .ok_or(ObjectCloseTxnStateError::IdentifierExhausted)?;
        self.next_attempt_id = attempt_id;
        let obligation = self
            .obligations
            .get_mut(&object_id)
            .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
        obligation.active_attempt = Some(attempt_id);
        Ok(CloseCleanupAttemptKey {
            obligation_id: authority.obligation_id,
            attempt_id,
        })
    }

    pub(crate) fn attempt_is_current(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        attempt: CloseCleanupAttemptKey,
    ) -> bool {
        self.obligations.get(&object_id).is_some_and(|obligation| {
            obligation.generation == generation
                && obligation.obligation_id == attempt.obligation_id
                && obligation.active_attempt == Some(attempt.attempt_id)
        })
    }

    pub(crate) fn finish_cleanup_attempt(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        attempt: CloseCleanupAttemptKey,
        complete: bool,
    ) -> Result<(), ObjectCloseTxnStateError> {
        if !self.attempt_is_current(object_id, generation, attempt) {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        }
        if complete {
            self.obligations.remove(&object_id);
        } else {
            let obligation = self
                .obligations
                .get_mut(&object_id)
                .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
            obligation.active_attempt = None;
        }
        Ok(())
    }

    pub fn is_idempotent_complete(
        runtime: &TunerServiceRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
    ) -> Result<bool, HalError> {
        let entry = runtime.object_table().entry(object_id).ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object is missing during close preflight",
            )
        })?;
        if entry.generation != generation || entry.object_kind != object_kind {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object identity changed during close preflight",
            ));
        }
        Ok(entry.lifecycle == crate::RuntimeObjectLifecycle::Closed
            && matches!(object_kind, AidlObjectKind::Frontend | AidlObjectKind::Lnb))
    }

    pub fn begin(
        runtime: &mut TunerServiceRuntime,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        object_kind: AidlObjectKind,
        method: AidlMethodCall,
    ) -> Result<ObjectCloseUseCasePlan, HalError> {
        begin_object_close_txn(runtime, object_id, generation, object_kind, method)
    }

    pub fn finish(
        runtime: &mut TunerServiceRuntime,
        completion: CloseCleanupAttemptCompletion,
        cleanup_result: Result<(), ObjectCloseCleanupFailure>,
    ) -> Result<(), HalError> {
        finish_object_close_txn(runtime, completion, cleanup_result)
    }
}

pub fn close_object_use_case(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    method: AidlMethodCall,
) -> Result<ObjectCloseUseCasePlan, HalError> {
    ObjectCloseTxn::begin(runtime, object_id, generation, object_kind, method)
}

fn begin_object_close_txn(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    object_kind: AidlObjectKind,
    method: AidlMethodCall,
) -> Result<ObjectCloseUseCasePlan, HalError> {
    let authority = plan_and_begin_object_close_method_call_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        method,
        CleanupStep::ReleaseBackend,
    )?;

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
        authority,
        cascade_entries,
        domain_cleanup_step: CleanupStep::ReleaseBackend,
        artifact_cleanup_commands,
        domain_cleanup_commands,
    })
}

pub fn finish_object_close_use_case(
    runtime: &mut TunerServiceRuntime,
    completion: CloseCleanupAttemptCompletion,
    cleanup_result: Result<(), ObjectCloseCleanupFailure>,
) -> Result<(), HalError> {
    ObjectCloseTxn::finish(runtime, completion, cleanup_result)
}

fn finish_object_close_txn(
    runtime: &mut TunerServiceRuntime,
    completion: CloseCleanupAttemptCompletion,
    cleanup_result: Result<(), ObjectCloseCleanupFailure>,
) -> Result<(), HalError> {
    let object_id = completion.object_id;
    let generation = completion.generation;
    if let Err(cleanup_failure) = cleanup_result {
        match runtime.object_table_mut().finish_close_cleanup_attempt(
            object_id,
            generation,
            completion.key,
            CloseCleanupAttemptOutcome::Pending {
                step: cleanup_failure.step(),
            },
        ).map_err(object_table_error_to_hal) {
            Ok(_) => return Err(cleanup_failure.into_error()),
            Err(mark_error) => {
                return Err(compose_primary_cleanup_failure(
                    "object close cleanup failed and cleanup-failed marking failed",
                    cleanup_failure.into_error(),
                    mark_error,
                ));
            }
        }
    }

    runtime
        .object_table_mut()
        .finish_close_cleanup_attempt(
            object_id,
            generation,
            completion.key,
            CloseCleanupAttemptOutcome::Complete,
        )
        .map(|_| ())
        .map_err(object_table_error_to_hal)
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
) -> Result<CloseCleanupAuthority, HalError> {
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
) -> Result<CloseCleanupAuthority, HalError> {
    match plan_object_close_method_dispatch(
        runtime,
        object_id,
        generation,
        object_kind,
        command_plan,
        executable_request,
    )? {
        AidlObjectCloseability::BeginClose => {
            let (_, key) = runtime
                .object_table_mut()
                .begin_close_cascade_with_cleanup_authority(object_id, generation, step)
                .map_err(object_table_error_to_hal)?;
            Ok(CloseCleanupAuthority {
                object_id,
                generation,
                key,
            })
        }
    }
}

#[cfg(test)]
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

    fn runtime_with_filter_for_close_attempt(object_id: i64) -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(object_id),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(object_id),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        runtime
    }

    fn begin_filter_close_plan(
        runtime: &mut TunerServiceRuntime,
        object_id: i64,
    ) -> ObjectCloseUseCasePlan {
        close_object_use_case(
            runtime,
            AidlObjectId(object_id),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
            AidlMethodCall::FilterClose,
        )
        .expect("filter close plan begins")
    }

    #[test]
    fn close_cleanup_authority_can_cross_the_reaper_thread_boundary() {
        fn assert_send<T: Send>() {}

        assert_send::<CloseCleanupAuthority>();
    }

    #[test]
    fn dropped_cleanup_authority_is_reissued_before_side_effects_start() {
        let mut runtime = runtime_with_filter_for_close_attempt(3);
        let first = begin_filter_close_plan(&mut runtime, 3);
        drop(first);

        let retry = begin_filter_close_plan(&mut runtime, 3);
        retry
            .begin_cleanup_attempt(&mut runtime)
            .expect("dropped authority leaves the obligation reissuable");
    }

    #[test]
    fn cleanup_attempt_fence_allows_only_one_side_effect_executor() {
        let mut runtime = runtime_with_filter_for_close_attempt(30);
        let first = begin_filter_close_plan(&mut runtime, 30);
        let competing = begin_filter_close_plan(&mut runtime, 30);

        first
            .begin_cleanup_attempt(&mut runtime)
            .expect("first cleanup attempt starts");
        assert!(competing.begin_cleanup_attempt(&mut runtime).is_err());
    }

    #[test]
    fn stale_cleanup_completion_cannot_finish_a_retried_attempt() {
        let mut runtime = runtime_with_filter_for_close_attempt(31);
        let first = begin_filter_close_plan(&mut runtime, 31)
            .begin_cleanup_attempt(&mut runtime)
            .expect("first cleanup attempt starts");
        let stale_completion = CloseCleanupAttemptCompletion {
            object_id: first.completion.object_id,
            generation: first.completion.generation,
            key: first.completion.key,
        };
        let first_result = finish_object_close_use_case(
            &mut runtime,
            first.completion,
            Err(ObjectCloseCleanupFailure::new(
                CleanupStep::ReleaseBackend,
                HalError::cleanup_failed("first cleanup attempt", "injected cleanup failure"),
            )),
        );
        assert!(first_result.is_err());

        let retry = begin_filter_close_plan(&mut runtime, 31)
            .begin_cleanup_attempt(&mut runtime)
            .expect("retry cleanup attempt starts");
        assert!(finish_object_close_use_case(&mut runtime, stale_completion, Ok(())).is_err());
        finish_object_close_use_case(&mut runtime, retry.completion, Ok(()))
            .expect("current cleanup attempt can finish");
    }

    #[test]
    fn finish_close_use_case_commits_after_successful_cleanup_report() {
        let mut runtime = runtime_with_filter_for_close_attempt(4);
        let close_plan = begin_filter_close_plan(&mut runtime, 4);
        let cleanup_attempt = close_plan
            .begin_cleanup_attempt(&mut runtime)
            .expect("cleanup attempt begins");
        let completion = cleanup_attempt.completion;

        finish_object_close_use_case(&mut runtime, completion, Ok(())).expect("finish succeeds");

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
        let mut runtime = runtime_with_filter_for_close_attempt(5);
        let close_plan = begin_filter_close_plan(&mut runtime, 5);
        let cleanup_attempt = close_plan
            .begin_cleanup_attempt(&mut runtime)
            .expect("cleanup attempt begins");
        let completion = cleanup_attempt.completion;

        let result = finish_object_close_use_case(
            &mut runtime,
            completion,
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
            crate::RuntimeObjectLifecycle::CleanupPending {
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
