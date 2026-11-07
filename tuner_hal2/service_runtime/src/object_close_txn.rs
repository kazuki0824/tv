use std::collections::{BTreeMap, BTreeSet};

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, HalError, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlMethodAdapter, AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    CommandPlan, RuntimeExecutableRequest,
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

/// DVR通知workerの回収権限がSupervisorへ移管済みで、他のcleanup工程と
/// 対象runtime登録解除が成功した場合だけ、局所的な回収待ちを許す。
/// 完全な物理cleanup成功ではなく、未完義務を保持した隔離の証拠である。
pub fn drop_leak_cleanup_supervised_targets(
    report: &ObjectCleanupExecutionReport,
) -> Option<Vec<(AidlObjectId, AidlObjectGeneration)>> {
    let mut pending_owners = Vec::new();
    let mut unregistered = None;
    for outcome in report.outcomes() {
        match outcome {
            ObjectCleanupStepOutcome::Artifact {
                kind: ObjectArtifactCleanupKind::DvrStatusNotifier,
                target,
                result: Err(failure),
                ..
            } if matches!(
                failure.error(),
                HalError::WorkerCleanupFailed {
                    kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Executing
                }
            ) =>
            {
                pending_owners.push(*target);
            }
            ObjectCleanupStepOutcome::Runtime {
                kind: ObjectRuntimeCleanupKind::DropLeakPublicRuntimeUnregister,
                cascade_entries,
                result: Ok(()),
            } => {
                unregistered = Some(cascade_entries.as_slice());
            }
            other if other.result().is_err() => return None,
            _ => {}
        }
    }
    if pending_owners.is_empty()
        || !unregistered.is_some_and(|entries| {
            pending_owners.iter().all(|target| {
                entries.iter().any(|entry| {
                    entry.object_kind == target.object_kind()
                        && entry.object_id == target.object_id()
                        && entry.generation == target.generation()
                })
            })
        })
    {
        return None;
    }
    let mut targets = pending_owners
        .into_iter()
        .map(|target| (target.object_id(), target.generation()))
        .collect::<Vec<_>>();
    targets.sort_unstable();
    targets.dedup();
    Some(targets)
}

/// 元の真偽判定を確認する単体テスト専用の入口。
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

/// 通知workerをSupervisorへ移管した後も、完了義務は既存ObjectCloseTxnが保持する。
/// 別の所有台帳は設けない。
#[derive(Clone, Debug, Eq, PartialEq)]
enum QuarantinedCleanupProgress {
    AwaitingReport(BTreeMap<(AidlObjectId, AidlObjectGeneration), bool>),
    AwaitingJoins(BTreeSet<(AidlObjectId, AidlObjectGeneration)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CloseCleanupObligation {
    generation: AidlObjectGeneration,
    obligation_id: u64,
    active_attempt: Option<u64>,
    quarantine: Option<QuarantinedCleanupProgress>,
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
            .begin_close_cleanup_attempt(authority.object_id, authority.generation, authority.key)
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
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    cascade_entries: Vec<RuntimeObjectEntry>,
    artifact_cleanup_commands: Vec<ObjectArtifactCleanupCommand>,
    domain_cleanup_commands: Vec<ObjectDomainCleanupCommand>,
}

#[must_use]
pub struct QuarantineCleanupReport {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    report: ObjectCleanupExecutionReport,
}

impl QuarantineCleanupReport {
    pub fn report(&self) -> &ObjectCleanupExecutionReport {
        &self.report
    }

    pub fn finish(self, runtime: &mut TunerServiceRuntime) -> Result<bool, HalError> {
        let supervised = drop_leak_cleanup_supervised_targets(&self.report);
        if supervised.is_none() {
            self.report.into_result().map_err(ObjectCloseCleanupFailure::into_error)?;
        }
        runtime
            .object_table_mut()
            .finish_quarantined_cleanup_report(
                self.object_id,
                self.generation,
                supervised.as_deref().unwrap_or(&[]),
            )
            .map_err(object_table_error_to_hal)?;
        Ok(supervised.is_some())
    }
}

impl ObjectDropLeakQuarantinePlan {
    pub fn execute_terminalization_report_with_executor<R, D, A>(
        self,
        runtime_executor: &mut R,
        domain_executor: &mut D,
        artifact_executor: &mut A,
    ) -> QuarantineCleanupReport
    where
        R: ObjectCloseRuntimeExecutor,
        D: ObjectDomainCleanupExecutor,
        A: ObjectArtifactCleanupExecutor,
    {
        let Self {
            object_id,
            generation,
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
        QuarantineCleanupReport { object_id, generation, report }
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

/// close時のcleanup責務と試行境界を永続的に保持する正本owner。
/// `RuntimeObjectTable`がprivate sub-ownerとして保持し、責務の変更はすべてこの入口へ委譲する。
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

    #[cfg(test)]
    pub(crate) fn exhaust_cleanup_ids_for_test(&mut self) {
        self.next_obligation_id = u64::MAX;
    }

    pub(crate) fn has_outstanding_obligation(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> bool {
        self.obligations
            .get(&object_id)
            .is_some_and(|obligation| obligation.generation == generation)
    }

    pub(crate) fn has_managed_quarantine(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> bool {
        self.obligations.get(&object_id).is_some_and(|obligation| {
            obligation.generation == generation && obligation.quarantine.is_some()
        })
    }

    pub(crate) fn begin_quarantined_cleanup(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        notifier_candidates: impl IntoIterator<Item = (AidlObjectId, AidlObjectGeneration)>,
    ) -> Result<(), ObjectCloseTxnStateError> {
        // 物理cleanupより前に正本へ義務を登録する。
        // 通知workerの回収がreportより先に終了しても完了通知を失わない。
        if self
            .obligations
            .get(&object_id)
            .is_some_and(|obligation| obligation.quarantine.is_some())
        {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        }
        let authority = self.issue_cleanup_authority(object_id, generation)?;
        let obligation = self
            .obligations
            .get_mut(&object_id)
            .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
        if obligation.obligation_id != authority.obligation_id {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        }
        obligation.quarantine = Some(QuarantinedCleanupProgress::AwaitingReport(
            notifier_candidates
                .into_iter()
                .map(|key| (key, false))
                .collect(),
        ));
        Ok(())
    }

    /// Supervisorに移管した通知worker以外の物理cleanupが成功した場合だけ呼ぶ。
    /// 全通知workerの回収終了後に限り完了を返す。
    pub(crate) fn accept_quarantine_cleanup_report(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        pending: &[(AidlObjectId, AidlObjectGeneration)],
    ) -> Result<bool, ObjectCloseTxnStateError> {
        let obligation = self
            .obligations
            .get_mut(&object_id)
            .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
        if obligation.generation != generation || obligation.active_attempt.is_some() {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        }
        let Some(QuarantinedCleanupProgress::AwaitingReport(candidates)) =
            obligation.quarantine.as_ref()
        else {
            return Err(ObjectCloseTxnStateError::InvalidAuthority);
        };
        let mut outstanding = BTreeSet::new();
        for key in pending {
            let joined = candidates
                .get(key)
                .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
            if !*joined {
                outstanding.insert(*key);
            }
        }
        let finished = outstanding.is_empty();
        if finished {
            self.obligations.remove(&object_id);
        } else {
            obligation.quarantine = Some(QuarantinedCleanupProgress::AwaitingJoins(outstanding));
        }
        Ok(finished)
    }

    /// join通知で未完義務が消えるownerを、破壊的変更の前に列挙する。
    /// ObjectTableはこの集合の全lifecycleを検証してから通知をcommitする。
    pub(crate) fn quarantine_roots_completing_on_join(
        &self,
        notifier: (AidlObjectId, AidlObjectGeneration),
    ) -> Vec<(AidlObjectId, AidlObjectGeneration)> {
        self.obligations
            .iter()
            .filter_map(|(object_id, obligation)| {
                let QuarantinedCleanupProgress::AwaitingJoins(pending) =
                    obligation.quarantine.as_ref()?
                else {
                    return None;
                };
                (pending.len() == 1 && pending.contains(&notifier))
                    .then_some((*object_id, obligation.generation))
            })
            .collect()
    }

    /// WorkerRuntimeSupervisorが対象workerをjoinした後だけ通知する。
    /// 同じ世代への重複通知から新たなcleanup義務を生成しない。
    pub(crate) fn notifier_joined(
        &mut self,
        notifier: (AidlObjectId, AidlObjectGeneration),
    ) -> Vec<(AidlObjectId, AidlObjectGeneration)> {
        let mut finished = Vec::new();
        for (object_id, obligation) in &mut self.obligations {
            match obligation.quarantine.as_mut() {
                Some(QuarantinedCleanupProgress::AwaitingReport(candidates)) => {
                    if let Some(joined) = candidates.get_mut(&notifier) {
                        *joined = true;
                    }
                }
                Some(QuarantinedCleanupProgress::AwaitingJoins(pending)) => {
                    if pending.remove(&notifier) && pending.is_empty() {
                        finished.push((*object_id, obligation.generation));
                    }
                }
                None => {}
            }
        }
        for (object_id, _) in &finished {
            self.obligations.remove(object_id);
        }
        finished
    }

    pub(crate) fn issue_cleanup_authority(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<CloseCleanupAuthorityKey, ObjectCloseTxnStateError> {
        if let Some(obligation) = self.obligations.get(&object_id) {
            if obligation.generation != generation
                || obligation.active_attempt.is_some()
                || obligation.quarantine.is_some()
            {
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
                quarantine: None,
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
            .cloned()
            .ok_or(ObjectCloseTxnStateError::InvalidAuthority)?;
        if obligation.generation != generation
            || obligation.obligation_id != authority.obligation_id
            || obligation.active_attempt.is_some()
            || obligation.quarantine.is_some()
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

    /// 現在進行中の物理cleanup実行権限を読み取る。世代を跨いだ一致は許可しない。
    pub(crate) fn has_active_cleanup_attempt(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> bool {
        self.obligations.get(&object_id).is_some_and(|obligation| {
            obligation.generation == generation && obligation.active_attempt.is_some()
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
        match runtime
            .object_table_mut()
            .finish_close_cleanup_attempt(
                object_id,
                generation,
                completion.key,
                CloseCleanupAttemptOutcome::Pending {
                    step: cleanup_failure.step(),
                },
            )
            .map_err(object_table_error_to_hal)
        {
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
        method_plan.executable_request.clone(),
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

/// 新たなcleanup権限を作らず、既存の回収責務を所有する対象を取得する。
pub fn related_cleanup_recovery_owners_use_case(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Vec<(AidlObjectKind, AidlObjectId, AidlObjectGeneration, bool)>, HalError> {
    runtime
        .object_table()
        .related_cleanup_owners(object_id, generation)
        .map(|owners| {
            owners
                .into_iter()
                .map(|(entry, active)| {
                    (entry.object_kind, entry.object_id, entry.generation, active)
                })
                .collect()
        })
        .map_err(object_table_error_to_hal)
}

pub fn quarantine_object_drop_leak_use_case(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<Option<ObjectDropLeakQuarantinePlan>, HalError> {
    // 通常closeはruntime lock外で物理cleanupを行う。ここで進行中の
    // ObjectCloseTxnの実行権限を奪って別のcleanupを走らせてはならない。
    // 確認とquarantine遷移は同じruntime lock内で実行される。
    if runtime
        .object_table()
        .has_active_cleanup_attempt_in_cascade(object_id, generation)
        .map_err(object_table_error_to_hal)?
    {
        // 競合を全体障害として報告しない。既存権限だけが物理cleanupを進める。
        return Ok(None);
    }
    if runtime
        .object_table()
        .has_related_outstanding_cleanup(object_id, generation)
        .map_err(object_table_error_to_hal)?
    {
        // 祖先・子孫の既存cleanup権限を優先し、重複する物理cleanupを開始しない。
        return Ok(None);
    }
    // artifact計画は読取り専用。失敗し得る権限発行より先に状態を変更しない。
    let candidates = runtime
        .object_table()
        .quarantine_candidate_entries(object_id, generation)
        .map_err(object_table_error_to_hal)?;
    let artifact_cleanup_commands =
        artifact_cleanup_commands_for_drop_leak_plan(runtime, &candidates);
    let notifier_candidates = artifact_cleanup_commands
        .iter()
        .filter(|command| command.kind == ObjectArtifactCleanupKind::DvrStatusNotifier)
        .map(|command| (command.object_id, command.generation));
    let cascade_entries = runtime
        .object_table_mut()
        .begin_quarantine_cleanup(object_id, generation, notifier_candidates)
        .map_err(object_table_error_to_hal)?;
    let domain_cleanup_commands = cascade_entries
        .iter()
        .filter_map(drop_leak_domain_cleanup_command_for_entry)
        .collect();
    Ok(Some(ObjectDropLeakQuarantinePlan {
        object_id,
        generation,
        cascade_entries,
        artifact_cleanup_commands,
        domain_cleanup_commands,
    }))
}

/// Supervisorが所有する通知workerを消費し、joinの成功からだけ完了権限を発行する。
#[must_use]
pub struct DvrNotifierJoinCompletion {
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
}

impl DvrNotifierJoinCompletion {
    pub fn join(
        worker: crate::WorkerRuntime<()>,
    ) -> (crate::ClassifiedWorkerTerminalResult<()>, Option<Self>) {
        let object_id = AidlObjectId(worker.owner_id());
        let generation = AidlObjectGeneration(worker.generation());
        let (terminal, completion) = match worker.join_with_result() {
            Ok(terminal) => (terminal, Some(Self { object_id, generation })),
            Err(error) => (error.into_terminal_result("DVR notifier"), None),
        };
        (
            crate::WorkerFailureClassifier::classify_terminal(
                terminal,
                "DVR通知workerをjoinできませんでした",
            ),
            completion,
        )
    }

    pub fn finish(self, runtime: &mut TunerServiceRuntime) -> Result<(), HalError> {
        runtime
            .object_table_mut()
            .notify_supervised_dvr_joined(self.object_id, self.generation)
            .map_err(object_table_error_to_hal)
    }
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
    fn failed_dvr_worker_can_prove_successful_join() {
        let worker = crate::WorkerRuntime::<()>::spawn(
            "dvr-join-regression".to_string(),
            17,
            2,
            |_| Err(HalError::cleanup_failed("DVR通知", "異常終了を注入")),
            || {},
        )
        .unwrap();
        let (terminal, completion) = DvrNotifierJoinCompletion::join(worker);
        assert!(matches!(
            terminal,
            crate::ClassifiedWorkerTerminalResult::Failure { .. }
        ));
        let completion = completion.expect("業務結果が失敗でもjoinは完了する");
        assert_eq!(completion.object_id, AidlObjectId(17));
        assert_eq!(completion.generation, AidlObjectGeneration(2));
    }

    #[test]
    fn product_capacity_saturates_after_real_open_close_before_reaper_group_release() {
        use crate::worker_runtime::{WorkerContext, WorkerRuntime};
        use std::sync::{mpsc, Arc, Condvar, Mutex};
        use std::time::{Duration, Instant};

        // テスト途中のpanicでも全runnerを必ず解放する。
        struct ReleaseGateOnDrop(Arc<(Mutex<bool>, Condvar)>);
        impl ReleaseGateOnDrop {
            fn release(&self) {
                let (lock, wake) = &*self.0;
                *lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                wake.notify_all();
            }
        }
        impl Drop for ReleaseGateOnDrop {
            fn drop(&mut self) {
                self.release();
            }
        }

        // 製品既定の公開能力・自動AIDL ID発行・正規Demux open/closeを使う。
        // runnerは実close完了を確定した後でgroup解放直前だけ同期的に停止する。
        let snapshot = crate::CapabilitySnapshot::product_default();
        snapshot.validate_dependency_closures().unwrap();
        let capacity = snapshot.cleanup_reaper_capacity;
        assert_eq!(capacity, 176);
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        assert_eq!(runtime.lock().unwrap().capability_snapshot(), snapshot);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release_gate = ReleaseGateOnDrop(Arc::clone(&gate));
        let worker_gate = Arc::clone(&gate);
        let worker_runtime = Arc::clone(&runtime);
        let (finished_tx, finished_rx) = mpsc::channel();
        let runner = Arc::new(
            move |(entry, completion): (RuntimeObjectEntry, CloseCleanupAttemptCompletion),
                  _: _,
                  _: WorkerContext| {
                {
                    let mut runtime = worker_runtime.lock().unwrap();
                    runtime
                        .validate_public_runtime_for_closed_aidl_entry(&entry)
                        .unwrap();
                    runtime
                        .unregister_public_runtime_for_closed_aidl_entry(&entry)
                        .unwrap();
                    finish_object_close_use_case(&mut runtime, completion, Ok(())).unwrap();
                    assert_eq!(
                        runtime
                            .object_table()
                            .entry(entry.object_id())
                            .unwrap()
                            .lifecycle,
                        crate::RuntimeObjectLifecycle::Closed
                    );
                }
                finished_tx
                    .send((entry.object_id(), entry.generation()))
                    .unwrap();
                // 正規runner終了後のpending.release_group直前でworkerが
                // スケジューリング停止する区間を、同期ゲートで再現する。
                let (lock, wake) = &*worker_gate;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            },
        );
        // Worker側のrunner参照が全て解放されたらlaneが正常に終了したと判断する。
        let weak_runner = Arc::downgrade(&runner);
        let reaper = WorkerRuntime::start_reaper_queue::<
            (AidlObjectId, AidlObjectGeneration),
            CleanupStep,
            (RuntimeObjectEntry, CloseCleanupAttemptCompletion),
        >(capacity, "product-demux-close-overlap", runner)
        .unwrap();
        let mut distinct_ids = std::collections::BTreeSet::new();
        let mut retained_keys = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            let (entry, completion) = {
                let mut runtime = runtime.lock().unwrap();
                let entry = runtime
                    .root_open_txn()
                    .open_demux_root_object(AidlMethodCall::PublicApi {
                        object: AidlObjectKind::Tuner,
                        api: AidlApi::TunerOpenDemux,
                    })
                    .expect("公開Demux枠を再獲得");
                let plan = close_object_use_case(
                    &mut runtime,
                    entry.object_id(),
                    entry.generation(),
                    AidlObjectKind::Demux,
                    AidlMethodCall::DemuxClose,
                )
                .expect("正規close権限の発行");
                let completion = plan
                    .begin_cleanup_attempt(&mut runtime)
                    .expect("cleanup実行権限の確保")
                    .completion;
                (entry, completion)
            };
            assert!(distinct_ids.insert(entry.object_id()));
            let key = (entry.object_id(), entry.generation());
            reaper
                .enqueue_reserved((entry, completion), [(key, CleanupStep::ReleaseBackend)])
                .expect("製品容量までは正規reaperへ受付");
            assert_eq!(
                finished_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                key
            );
            assert!(reaper.pending_value(&key).unwrap().is_some());
            retained_keys.push(key);
        }
        // 全176件はpublic同時Liveではない。実open/closeでClosed確定済み、
        // ただしworker内に留まる間は同世代の正規groupを返却できない。
        let extra = {
            let mut runtime = runtime.lock().unwrap();
            runtime
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .expect("旧世代は閉じているため次のDemuxをopenできる")
        };
        assert!(distinct_ids.insert(extra.object_id()));
        let capacity_error = reaper
            .reserve_pending([(
                (extra.object_id(), extra.generation()),
                CleanupStep::ReleaseBackend,
            )])
            .err()
            .expect("正規openから得た世代でも容量上限を超える");
        assert!(matches!(
            capacity_error,
            maleicacid_tuner_hal2_common::HalError::Busy { .. }
        ));
        release_gate.release();

        // 予約groupがworkerのrun完了後に全件返却されるまで有界に待機。
        let deadline = Instant::now() + Duration::from_secs(10);
        for key in &retained_keys {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "製品reaperの予約groupを期限内に全件解放できませんでした"
            );
            reaper
                .wait_until_released([key], Some(remaining))
                .expect("workerの予約groupは期限内に解放される");
        }
        assert!(retained_keys
            .iter()
            .all(|key| { reaper.pending_value(key).unwrap().is_none() }));
        // Senderを閉じて各laneのrecvを終了させる。Weak runnerが失効するまで
        // 待つことで、テスト用のworkerを残さず終了したことを確認する。
        drop(reaper);
        let shutdown_deadline = Instant::now() + Duration::from_secs(10);
        while weak_runner.upgrade().is_some() && Instant::now() < shutdown_deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            weak_runner.upgrade().is_none(),
            "製品reaperのworkerが期限内に終了しませんでした"
        );
    }

    #[test]
    fn begin_close_cascade_moves_live_object_to_closing() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                last_reference_ended: false,
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
                last_reference_ended: false,
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
                last_reference_ended: false,
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
    fn demux_close_unregisters_runtime_while_start_guard_is_active() {
        use crate::boot::{FrontendProbeOutcome, ServiceBootOutcome};
        use crate::registry::{
            FrontendCapabilitySnapshot, FrontendRuntimeId, FrontendScalarCapability,
            SatellitePowerTopology,
        };
        use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
        use maleicacid_tuner_hal2_demux::{FilterOpenType, OpenFilterRequest};

        let frontend_id = 1_000_000;
        let mut runtime = TunerServiceRuntime::new();
        assert_eq!(
            runtime.boot_from_probe_results([FrontendProbeOutcome::Available {
                id: FrontendRuntimeId(frontend_id),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbT,
                path: "/dev/px4video0".into(),
                lnb_profile: None,
                satellite_power_topology: SatellitePowerTopology::UnknownOrDisabled,
                capability: FrontendCapabilitySnapshot {
                    scalar: FrontendScalarCapability {
                        min_frequency_hz: 110_642_857,
                        max_frequency_hz: 767_642_857,
                        min_symbol_rate: 0,
                        max_symbol_rate: 0,
                        acquire_range_hz: 0,
                    },
                    exclusive_group_id: 0x1000_0000,
                    isdbt_segment: Some(crate::registry::IsdbtSegmentCapability {
                        is_segment_auto: true,
                        is_full_segment: true,
                    }),
                },
            }]),
            ServiceBootOutcome::Ready,
        );

        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &OpenFilterRequest {
                    open_type: FilterOpenType::TsRaw,
                    buffer_size: 4096,
                    callback_present: false,
                },
            )
            .unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, frontend_id)
            .unwrap();

        let demux_object_id = AidlObjectId(90);
        let filter_object_id = AidlObjectId(91);
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Demux,
                object_id: demux_object_id,
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(i64::from(demux.id.0)),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .unwrap();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Filter,
                object_id: filter_object_id,
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(i64::from(filter.id.0)),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Demux {
                    demux: demux_object_id,
                    generation: AidlObjectGeneration(1),
                },
                lifecycle: crate::RuntimeObjectLifecycle::Live,
            })
            .unwrap();

        let start_guard = runtime
            .try_begin_frontend_demux_start(frontend_id)
            .unwrap()
            .unwrap();

        let first = close_object_use_case(
            &mut runtime,
            demux_object_id,
            AidlObjectGeneration(1),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .unwrap()
        .begin_cleanup_attempt(&mut runtime)
        .unwrap();

        // START一回性guardはrelation変更やcloseの拒否条件ではない。
        unregister_public_runtime_entries_for_close(&mut runtime, &first.cascade_entries)
            .expect("START中もdemuxと子filterを解除できる");
        assert!(runtime.registry().demux(demux.id).is_none());
        assert!(runtime.registry().filter(filter.id).is_none());
        assert!(runtime
            .registry()
            .frontend_bound_demux_ids(FrontendRuntimeId(frontend_id))
            .is_empty());
        finish_object_close_use_case(&mut runtime, first.completion, Ok(()))
            .expect("close所有者が終端を確定する");
        assert_eq!(
            runtime
                .object_table()
                .entry(demux_object_id)
                .unwrap()
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closed
        );
        assert_eq!(
            runtime
                .object_table()
                .entry(filter_object_id)
                .unwrap()
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closed
        );
        start_guard.release();
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
        let _attempt = retry
            .begin_cleanup_attempt(&mut runtime)
            .expect("dropped authority leaves the obligation reissuable");
    }

    #[test]
    fn drop_leak_quarantine_cannot_preempt_an_active_close_cleanup() {
        let mut runtime = runtime_with_filter_for_close_attempt(30);
        let first = begin_filter_close_plan(&mut runtime, 30)
            .begin_cleanup_attempt(&mut runtime)
            .expect("既存closeがcleanup実行権限を獲得する");
        let failure_state = runtime.failure_state();
        assert!(quarantine_object_drop_leak_use_case(
            &mut runtime,
            AidlObjectId(30),
            AidlObjectGeneration(1)
        )
        .expect("既存cleanupの権限競合は異常ではない")
        .is_none());
        assert!(!failure_state.snapshot().service_critical);
        assert!(matches!(
            runtime
                .object_table()
                .entry(AidlObjectId(30))
                .unwrap()
                .lifecycle,
            crate::RuntimeObjectLifecycle::Closing { .. }
        ));

        let result = finish_object_close_use_case(
            &mut runtime,
            first.completion,
            Err(ObjectCloseCleanupFailure::new(
                CleanupStep::ReleaseBackend,
                HalError::cleanup_failed("テスト", "先行cleanup失敗"),
            )),
        );
        assert!(result.is_err());
        quarantine_object_drop_leak_use_case(
            &mut runtime,
            AidlObjectId(30),
            AidlObjectGeneration(1),
        )
        .expect("先行cleanup実行権限解放後は隔離できる")
        .expect("隔離planが発行される");
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(30))
                .unwrap()
                .lifecycle,
            crate::RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn issued_close_authority_cannot_start_after_quarantine_handoff() {
        let object_id = AidlObjectId(450);
        let generation = AidlObjectGeneration(1);
        let mut txn = ObjectCloseTxn::new();
        let authority = txn
            .issue_cleanup_authority(object_id, generation)
            .expect("close権限を取得する");
        txn.begin_quarantined_cleanup(object_id, generation, [])
            .expect("外部cleanup開始前に隔離権限へ移管する");
        assert!(
            txn.begin_cleanup_attempt(object_id, generation, authority)
                .is_err(),
            "隔離後に古いclose権限で物理cleanupを再開始してはならない"
        );
        assert!(txn.has_outstanding_obligation(object_id, generation));
    }

    #[test]
    fn quarantine_handoff_completion_is_order_independent() {
        let root = AidlObjectId(430);
        let generation = AidlObjectGeneration(1);
        let notifier = (AidlObjectId(431), generation);
        let mut txn = ObjectCloseTxn::new();
        txn.begin_quarantined_cleanup(root, generation, [notifier])
            .expect("回収対象を一度だけ登録");
        assert!(!txn
            .accept_quarantine_cleanup_report(root, generation, &[notifier])
            .expect("後片付けreportを登録"));
        assert!(txn.has_outstanding_obligation(root, generation));
        assert!(txn.issue_cleanup_authority(root, generation).is_err());
        assert_eq!(txn.notifier_joined(notifier), vec![(root, generation)]);
        assert!(!txn.has_outstanding_obligation(root, generation));
        assert!(txn.notifier_joined(notifier).is_empty());

        // Supervisorの回収完了通知が物理cleanup reportより先に到着する場合。
        let mut txn = ObjectCloseTxn::new();
        txn.begin_quarantined_cleanup(root, generation, [notifier])
            .expect("次のテストownerを登録");
        assert!(txn.notifier_joined(notifier).is_empty());
        assert!(txn.has_outstanding_obligation(root, generation));
        assert!(txn
            .accept_quarantine_cleanup_report(root, generation, &[notifier])
            .expect("先に届いたjoinを保持して完了を認める"));
        assert!(!txn.has_outstanding_obligation(root, generation));
    }

    #[test]
    fn quarantine_handoff_cannot_be_finished_by_a_different_generation() {
        let root = AidlObjectId(440);
        let generation = AidlObjectGeneration(1);
        let notifier = (AidlObjectId(441), generation);
        let mut txn = ObjectCloseTxn::new();
        txn.begin_quarantined_cleanup(root, generation, [notifier])
            .expect("後片付けowner登録");
        assert!(!txn
            .accept_quarantine_cleanup_report(root, generation, &[notifier])
            .expect("Worker回収を待つ"));
        assert!(txn
            .notifier_joined((AidlObjectId(441), AidlObjectGeneration(2)))
            .is_empty());
        assert!(txn.has_outstanding_obligation(root, generation));
        assert_eq!(txn.notifier_joined(notifier), vec![(root, generation)]);
        assert!(!txn.has_outstanding_obligation(root, generation));
    }

    #[test]
    fn supervised_notifier_handoff_requires_matching_unregister_and_no_other_failures() {
        let pending = ObjectCleanupStepOutcome::Artifact {
            kind: ObjectArtifactCleanupKind::DvrStatusNotifier,
            target: ObjectCleanupObjectTarget::new(
                AidlObjectKind::Dvr,
                AidlObjectId(17),
                AidlObjectGeneration(2),
            ),
            step: CleanupStep::StopWorker,
            result: Err(ObjectCloseCleanupFailure::new(
                CleanupStep::StopWorker,
                HalError::WorkerCleanupFailed {
                    kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Executing,
                },
            )),
        };
        let mut report = ObjectCleanupExecutionReport::new();
        report.push(pending);
        assert_eq!(drop_leak_cleanup_supervised_targets(&report), None);

        let mut safe = ObjectCleanupExecutionReport::new();
        safe.push(report.outcomes()[0].clone());
        safe.push(ObjectCleanupStepOutcome::Runtime {
            kind: ObjectRuntimeCleanupKind::DropLeakPublicRuntimeUnregister,
            cascade_entries: vec![RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Dvr,
                object_id: AidlObjectId(17),
                generation: AidlObjectGeneration(2),
                ledger_id: LedgerId(17),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: crate::RuntimeObjectLifecycle::Quarantined,
            }],
            result: Ok(()),
        });
        assert_eq!(
            drop_leak_cleanup_supervised_targets(&safe),
            Some(vec![(AidlObjectId(17), AidlObjectGeneration(2))])
        );
        safe.push(ObjectCleanupStepOutcome::Artifact {
            kind: ObjectArtifactCleanupKind::OwnerCallbackRegistration,
            target: ObjectCleanupObjectTarget::new(
                AidlObjectKind::Dvr,
                AidlObjectId(17),
                AidlObjectGeneration(2),
            ),
            step: CleanupStep::UnregisterRuntime,
            result: Err(ObjectCloseCleanupFailure::new(
                CleanupStep::UnregisterRuntime,
                HalError::cleanup_failed("callback store", "解除失敗"),
            )),
        });
        assert_eq!(drop_leak_cleanup_supervised_targets(&safe), None);
    }

    #[test]
    fn cleanup_attempt_fence_allows_only_one_side_effect_executor() {
        let mut runtime = runtime_with_filter_for_close_attempt(30);
        let first = begin_filter_close_plan(&mut runtime, 30);
        let competing = begin_filter_close_plan(&mut runtime, 30);

        let _attempt = first
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
    fn closed_filter_is_not_idempotent_complete() {
        let mut runtime = runtime_with_filter_for_close_attempt(41);
        let close_plan = begin_filter_close_plan(&mut runtime, 41);
        let cleanup_attempt = close_plan
            .begin_cleanup_attempt(&mut runtime)
            .expect("cleanup試行開始が成功する");
        finish_object_close_use_case(&mut runtime, cleanup_attempt.completion, Ok(()))
            .expect("filter closeが成功する");

        assert!(!ObjectCloseTxn::is_idempotent_complete(
            &runtime,
            AidlObjectId(41),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .expect("Closed filterのidentityを引き続き読める"));
    }

    #[test]
    fn finish_close_use_case_commits_after_successful_cleanup_report() {
        let mut runtime = runtime_with_filter_for_close_attempt(4);
        let close_plan = begin_filter_close_plan(&mut runtime, 4);
        let cleanup_attempt = close_plan
            .begin_cleanup_attempt(&mut runtime)
            .expect("cleanup試行開始が成功する");
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
            .expect("cleanup試行開始が成功する");
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
                last_reference_ended: false,
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

#[cfg(test)]
mod frontend_root_reopen_tests {
    use super::*;
    use crate::boot::{FrontendProbeOutcome, ServiceBootOutcome};
    use crate::registry::{
        FrontendCapabilitySnapshot, FrontendRuntimeId, FrontendScalarCapability,
        SatellitePowerTopology,
    };
    use crate::RuntimeObjectLifecycleSnapshot;
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
    use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlObjectKind};
    use maleicacid_tuner_hal2_resource_ledger::CleanupStep;

    fn open_method() -> AidlMethodCall {
        AidlMethodCall::PublicApi {
            object: AidlObjectKind::Tuner,
            api: AidlApi::TunerOpenFrontendById,
        }
    }

    fn runtime_with_frontend(frontend_id: i32) -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        assert_eq!(
            runtime.boot_from_probe_results([FrontendProbeOutcome::Available {
                id: FrontendRuntimeId(frontend_id),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbT,
                path: "/dev/null".into(),
                lnb_profile: None,
                satellite_power_topology: SatellitePowerTopology::UnknownOrDisabled,
                capability: FrontendCapabilitySnapshot {
                    scalar: FrontendScalarCapability {
                        min_frequency_hz: 473_142_857,
                        max_frequency_hz: 473_142_857,
                        min_symbol_rate: 0,
                        max_symbol_rate: 0,
                        acquire_range_hz: 0,
                    },
                    exclusive_group_id: 0x1000_0007,
                    isdbt_segment: Some(crate::registry::IsdbtSegmentCapability {
                        is_segment_auto: true,
                        is_full_segment: true,
                    }),
                },
            }]),
            ServiceBootOutcome::Ready
        );
        runtime
    }

    #[test]
    fn active_frontend_lease_stays_unavailable_and_has_typed_diagnostic() {
        let frontend_id = 7;
        let mut runtime = runtime_with_frontend(frontend_id);
        let first = runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(frontend_id, open_method())
            .expect("最初のfrontend openが成功する");

        assert!(matches!(
            runtime
                .root_open_txn()
                .open_frontend_root_object_for_id(frontend_id, open_method()),
            Err(HalError::UnsupportedDetail {
                feature: "frontend.open",
                ..
            })
        ));

        let occupant = runtime
            .runtime_object_diagnostic_snapshots()
            .into_iter()
            .find(|snapshot| {
                snapshot.object_kind() == AidlObjectKind::Frontend
                    && snapshot.public_runtime_id().0 == i64::from(frontend_id)
            })
            .expect("active frontend occupantが診断に存在する");
        assert_eq!(occupant.object_id(), first.object_id());
        assert_eq!(occupant.generation(), first.generation());
        assert_eq!(occupant.lifecycle(), RuntimeObjectLifecycleSnapshot::Live);
    }

    #[test]
    fn cleanup_pending_frontend_reopens_only_after_close_owner_completion() {
        let frontend_id = 8;
        let mut runtime = runtime_with_frontend(frontend_id);
        let first = runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(frontend_id, open_method())
            .expect("最初のfrontend openが成功する");

        let first_close = crate::close_object_use_case(
            &mut runtime,
            first.object_id(),
            first.generation(),
            AidlObjectKind::Frontend,
            AidlMethodCall::FrontendClose,
        )
        .expect("close plan開始が成功する");
        let first_attempt = first_close
            .begin_cleanup_attempt(&mut runtime)
            .expect("最初のcleanup試行開始が成功する");
        crate::finish_object_close_use_case(
            &mut runtime,
            first_attempt.completion,
            Err(crate::ObjectCloseCleanupFailure::new(
                CleanupStep::ReleaseBackend,
                HalError::cleanup_failed(
                    "Issue #181の遅延cleanup",
                    "worker cleanupは引き続きreaperが所有しています",
                ),
            )),
        )
        .expect_err("未完了cleanupがpendingのまま残る");

        assert!(matches!(
            runtime
                .root_open_txn()
                .open_frontend_root_object_for_id(frontend_id, open_method()),
            Err(HalError::UnsupportedDetail {
                feature: "frontend.open",
                ..
            })
        ));
        assert!(runtime
            .runtime_object_diagnostic_snapshots()
            .iter()
            .any(|snapshot| {
                snapshot.object_id() == first.object_id()
                    && matches!(
                        snapshot.lifecycle(),
                        RuntimeObjectLifecycleSnapshot::CleanupPending { .. }
                    )
            }));

        let retry_close = crate::close_object_use_case(
            &mut runtime,
            first.object_id(),
            first.generation(),
            AidlObjectKind::Frontend,
            AidlMethodCall::FrontendClose,
        )
        .expect("既存close ownerがretryを受理する");
        let retry_attempt = retry_close
            .begin_cleanup_attempt(&mut runtime)
            .expect("retry cleanup試行開始が成功する");
        crate::finish_object_close_use_case(&mut runtime, retry_attempt.completion, Ok(()))
            .expect("既存close ownerがClosedへ到達する");

        let reopened = runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(frontend_id, open_method())
            .expect("Closed後にfrontendを再openできる");
        assert_ne!(reopened.object_id(), first.object_id());
    }

    #[test]
    fn quarantined_frontend_remains_fail_closed() {
        let frontend_id = 9;
        let mut runtime = runtime_with_frontend(frontend_id);
        let first = runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(frontend_id, open_method())
            .expect("最初のfrontend openが成功する");
        runtime
            .object_table_mut()
            .quarantine_cascade(first.object_id(), first.generation())
            .expect("quarantineが成功する");

        assert!(matches!(
            runtime
                .root_open_txn()
                .open_frontend_root_object_for_id(frontend_id, open_method()),
            Err(HalError::UnsupportedDetail {
                feature: "frontend.open",
                ..
            })
        ));
    }
}
