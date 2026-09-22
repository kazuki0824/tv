//! frontend 非同期worker slot所有。
//!
//! このmoduleは並行処理境界だけを所有する。tune/scan成功を装わず、呼び出し元がbackend jobを渡し、slotは完了・取消状態だけを記録する。
//! worker slotは完了・取消・失敗状態だけを保持し、実operationの成功を代用しない。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{FrontendTuneRequest, HalError, HalInternalKind};
use maleicacid_tuner_hal2_control_core::{
    WorkerCleanupAuthority, WorkerCleanupProgress, WorkerCleanupRun, WorkerContext, WorkerExit,
    WorkerFailureDomain, WorkerRuntime, WorkerRuntimeCleanup, WorkerStopReason,
    WorkerTerminalResult,
};

use super::backend_worker::{
    FrontendBackendSession, FrontendBackendSubmitFailure, FrontendBackendSubmitTicket,
    FrontendBackendSubmitWait, FrontendBackendTunePlan,
};
use crate::runtime::thread_result_owner::ThreadResultOwner;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum FrontendWorkerKind {
    Tune,
    Scan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCancelReason {
    StopRequested,
    SupersededByNewRequest,
    FrontendClosing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FrontendWorkerKey {
    pub frontend_id: i32,
    pub kind: FrontendWorkerKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendWorkerStartError {
    AlreadyRunning {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
    },
    CompletedFailurePending {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        exit: WorkerExit,
        error: HalError,
    },
    SpawnFailed {
        error: HalError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendWorkerStopOutcome {
    BackendSubmitFailed {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        failure: FrontendBackendSubmitFailure,
    },
    NotRunning,
    CancelRequested {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    },
    StopRequestFailed {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        reason: FrontendWorkerCancelReason,
        error: HalError,
    },
    Completed {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        exit: WorkerExit,
        result: Result<(), HalError>,
    },
}

impl FrontendWorkerStopOutcome {
    fn backend_submit_completion(
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        result: Result<(), FrontendBackendSubmitFailure>,
    ) -> Self {
        match result {
            Ok(()) => Self::Completed {
                frontend_id,
                kind,
                generation,
                exit: WorkerExit::Normal,
                result: Ok(()),
            },
            Err(failure) => Self::BackendSubmitFailed {
                frontend_id,
                kind,
                generation,
                failure,
            },
        }
    }
}

#[derive(Debug)]
struct FrontendWorkerDetachedJoin {
    frontend_id: i32,
    kind: FrontendWorkerKind,
    generation: u64,
    slot: FrontendWorkerSlot,
}

impl FrontendWorkerDetachedJoin {
    fn complete(&mut self) -> FrontendWorkerStopOutcome {
        let (result, exit) = self.slot.join_after_cancel();
        FrontendWorkerStopOutcome::Completed {
            frontend_id: self.frontend_id,
            kind: self.kind,
            generation: self.generation,
            exit,
            result,
        }
    }

    fn try_complete(&mut self) -> Option<FrontendWorkerStopOutcome> {
        let (result, exit) = self.slot.completed_result()?;
        Some(FrontendWorkerStopOutcome::Completed {
            frontend_id: self.frontend_id,
            kind: self.kind,
            generation: self.generation,
            exit,
            result,
        })
    }

    fn wait_until_finished(&self, deadline: Option<std::time::Instant>) -> Result<bool, HalError> {
        if self.slot.pending_completed.is_some() {
            return Ok(true);
        }
        self.slot
            .thread_result
            .as_ref()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker slot missing thread result owner while waiting",
                )
            })?
            .wait_until_finished(deadline)
    }
}

#[derive(Debug)]
struct FrontendBackendSubmitDetachedJoin {
    frontend_id: i32,
    kind: FrontendWorkerKind,
    generation: u64,
    ticket: FrontendBackendSubmitTicket,
}

impl FrontendBackendSubmitDetachedJoin {
    fn complete(&mut self) -> FrontendWorkerStopOutcome {
        FrontendWorkerStopOutcome::backend_submit_completion(
            self.frontend_id,
            self.kind,
            self.generation,
            self.ticket.complete_cleanup(),
        )
    }

    fn try_complete(&mut self) -> Option<FrontendWorkerStopOutcome> {
        let result = self.ticket.try_complete_cleanup()?;
        Some(FrontendWorkerStopOutcome::backend_submit_completion(
            self.frontend_id,
            self.kind,
            self.generation,
            result,
        ))
    }

    fn wait_until_finished(&self, deadline: Option<std::time::Instant>) -> Result<bool, HalError> {
        self.ticket.wait_until_cleanup(deadline)
    }
}

#[derive(Debug)]
enum FrontendWorkerCleanup {
    Join(FrontendWorkerDetachedJoin),
    BackendSubmitPrepared {
        plan: FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    },
    BackendSubmitJoin(FrontendBackendSubmitDetachedJoin),
    Failed(FrontendWorkerStopOutcome),
}

impl FrontendWorkerCleanup {
    fn finish(
        &mut self,
        outcome: FrontendWorkerStopOutcome,
    ) -> WorkerCleanupProgress<FrontendWorkerStopOutcome> {
        let succeeded = match &outcome {
            FrontendWorkerStopOutcome::Completed { result, .. } => result.is_ok(),
            FrontendWorkerStopOutcome::BackendSubmitFailed { failure, .. } => {
                failure.rollback_succeeded
            }
            _ => false,
        };
        if succeeded {
            WorkerCleanupProgress::Completed(outcome)
        } else {
            *self = Self::Failed(outcome.clone());
            WorkerCleanupProgress::Quarantined(outcome)
        }
    }

    fn complete(&mut self) -> WorkerCleanupProgress<FrontendWorkerStopOutcome> {
        let outcome = match self {
            Self::BackendSubmitPrepared { .. } => {
                return WorkerCleanupProgress::Completed(FrontendWorkerStopOutcome::NotRunning);
            }
            Self::Join(join) => join.complete(),
            Self::BackendSubmitJoin(join) => join.complete(),
            Self::Failed(outcome) => outcome.clone(),
        };
        self.finish(outcome)
    }

    fn try_complete(&mut self) -> WorkerCleanupProgress<FrontendWorkerStopOutcome> {
        let outcome = match self {
            Self::BackendSubmitPrepared { .. } => {
                return WorkerCleanupProgress::Completed(FrontendWorkerStopOutcome::NotRunning);
            }
            Self::Join(join) => join.try_complete(),
            Self::BackendSubmitJoin(join) => join.try_complete(),
            Self::Failed(outcome) => Some(outcome.clone()),
        };
        match outcome {
            Some(outcome) => self.finish(outcome),
            None => WorkerCleanupProgress::Pending,
        }
    }
}

#[derive(Debug)]
enum FrontendWorkerStopTicketKind {
    Immediate(FrontendWorkerStopOutcome),
    Retained {
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        authority: WorkerCleanupAuthority<FrontendWorkerCleanup>,
    },
}

#[derive(Debug)]
#[must_use = "frontend worker stop ticket must be completed or transferred to the reaper"]
pub struct FrontendWorkerStopTicket {
    kind: FrontendWorkerStopTicketKind,
}

#[derive(Debug)]
pub enum FrontendWorkerStopPoll {
    Pending(FrontendWorkerStopTicket),
    Completed(FrontendWorkerStopOutcome),
}

#[derive(Debug)]
pub enum FrontendWorkerSubmitWait {
    Completed(Result<FrontendBackendSession, FrontendBackendSubmitFailure>),
    TimedOut(FrontendWorkerStopTicket),
}

fn cleanup_authority_failure(
    frontend_id: i32,
    kind: FrontendWorkerKind,
    generation: u64,
    error: HalError,
) -> FrontendWorkerStopOutcome {
    let exit = if matches!(
        error,
        HalError::WorkerCleanupFailed {
            kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Interrupted,
        }
    ) {
        WorkerExit::PanicOrJoinFailure
    } else {
        WorkerExit::RuntimeFailure(WorkerFailureDomain::Signal.runtime_failure_kind())
    };
    FrontendWorkerStopOutcome::Completed {
        frontend_id,
        kind,
        generation,
        exit,
        result: Err(error),
    }
}

impl FrontendWorkerStopTicket {
    pub fn submit_until(
        self,
        deadline: std::time::Instant,
    ) -> Result<FrontendWorkerSubmitWait, HalError> {
        self.submit_until_with(deadline, FrontendBackendSubmitTicket::start)
    }

    fn submit_until_with(
        self,
        deadline: std::time::Instant,
        start: impl FnOnce(FrontendBackendTunePlan, Option<FrontendTuneRequest>)
            -> Result<FrontendBackendSubmitTicket, HalError>,
    ) -> Result<FrontendWorkerSubmitWait, HalError> {
        let FrontendWorkerStopTicketKind::Retained {
            frontend_id,
            kind,
            generation,
            authority,
        } = self.kind else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend backend submit has no retained authority",
            ));
        };
        let result = authority.execute(|value| {
            // 外部処理より先に結果不明の義務を置く。巻き戻し完了を観測するまで解除しない。
            let unknown = FrontendBackendSubmitFailure::indeterminate(
                generation,
                HalError::WorkerCleanupFailed {
                    kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Interrupted,
                },
            );
            let prepared = std::mem::replace(
                value,
                FrontendWorkerCleanup::Failed(FrontendWorkerStopOutcome::BackendSubmitFailed {
                    frontend_id,
                    kind,
                    generation,
                    failure: unknown.clone(),
                }),
            );
            let FrontendWorkerCleanup::BackendSubmitPrepared { plan, previous_request } = prepared else {
                *value = prepared;
                return WorkerCleanupProgress::Quarantined(Err(unknown));
            };
            let ticket = match start(plan, previous_request) {
                Ok(ticket) => ticket,
                Err(error) => {
                    // スレッド生成失敗では機器処理は開始されていない。
                    let mut failure = FrontendBackendSubmitFailure::indeterminate(generation, error);
                    failure.rollback_succeeded = true;
                    return WorkerCleanupProgress::Completed(Err(failure));
                }
            };
            match ticket.wait_until(deadline) {
                Ok(FrontendBackendSubmitWait::Completed(result)) => {
                    if let Err(failure) = &result {
                        if !failure.rollback_succeeded {
                            *value = FrontendWorkerCleanup::Failed(
                                FrontendWorkerStopOutcome::BackendSubmitFailed {
                                    frontend_id,
                                    kind,
                                    generation,
                                    failure: failure.clone(),
                                },
                            );
                            return WorkerCleanupProgress::Quarantined(result);
                        }
                    }
                    WorkerCleanupProgress::Completed(result)
                }
                Ok(FrontendBackendSubmitWait::TimedOut(ticket)) => {
                    *value = FrontendWorkerCleanup::BackendSubmitJoin(FrontendBackendSubmitDetachedJoin {
                        frontend_id,
                        kind,
                        generation,
                        ticket,
                    });
                    WorkerCleanupProgress::Pending
                }
                Err(error) => {
                    let failure = FrontendBackendSubmitFailure::indeterminate(generation, error);
                    *value = FrontendWorkerCleanup::Failed(FrontendWorkerStopOutcome::BackendSubmitFailed {
                        frontend_id,
                        kind,
                        generation,
                        failure: failure.clone(),
                    });
                    WorkerCleanupProgress::Quarantined(Err(failure))
                }
            }
        })?;
        Ok(match result {
            WorkerCleanupRun::Completed(result) => FrontendWorkerSubmitWait::Completed(result),
            WorkerCleanupRun::Pending(authority) => FrontendWorkerSubmitWait::TimedOut(Self {
                kind: FrontendWorkerStopTicketKind::Retained {
                    frontend_id,
                    kind,
                    generation,
                    authority,
                },
            }),
        })
    }

    fn immediate(outcome: FrontendWorkerStopOutcome) -> Self {
        Self {
            kind: FrontendWorkerStopTicketKind::Immediate(outcome),
        }
    }

    pub fn worker_generation(&self) -> Option<u64> {
        match &self.kind {
            FrontendWorkerStopTicketKind::Immediate(FrontendWorkerStopOutcome::NotRunning) => None,
            FrontendWorkerStopTicketKind::Immediate(
                FrontendWorkerStopOutcome::CancelRequested { generation, .. }
                | FrontendWorkerStopOutcome::Completed { generation, .. }
                | FrontendWorkerStopOutcome::BackendSubmitFailed { generation, .. }
                | FrontendWorkerStopOutcome::StopRequestFailed { generation, .. },
            )
            | FrontendWorkerStopTicketKind::Retained { generation, .. } => Some(*generation),
        }
    }

    pub fn complete(self) -> FrontendWorkerStopOutcome {
        match self.kind {
            FrontendWorkerStopTicketKind::Immediate(outcome) => outcome,
            FrontendWorkerStopTicketKind::Retained {
                frontend_id,
                kind,
                generation,
                authority,
            } => match authority.execute(FrontendWorkerCleanup::complete) {
                Ok(WorkerCleanupRun::Completed(outcome)) => outcome,
                Ok(WorkerCleanupRun::Pending(_)) => cleanup_authority_failure(
                    frontend_id,
                    kind,
                    generation,
                    HalError::cleanup_failed(
                        "frontend worker",
                        "blocking completion returned pending",
                    ),
                ),
                Err(error) => cleanup_authority_failure(frontend_id, kind, generation, error),
            },
        }
    }

    pub fn try_complete(self) -> FrontendWorkerStopPoll {
        match self.kind {
            FrontendWorkerStopTicketKind::Immediate(outcome) => {
                FrontendWorkerStopPoll::Completed(outcome)
            }
            FrontendWorkerStopTicketKind::Retained {
                frontend_id,
                kind,
                generation,
                authority,
            } => match authority.execute(FrontendWorkerCleanup::try_complete) {
                Ok(WorkerCleanupRun::Completed(outcome)) => {
                    FrontendWorkerStopPoll::Completed(outcome)
                }
                Ok(WorkerCleanupRun::Pending(authority)) => FrontendWorkerStopPoll::Pending(Self {
                    kind: FrontendWorkerStopTicketKind::Retained {
                        frontend_id,
                        kind,
                        generation,
                        authority,
                    },
                }),
                Err(error) => FrontendWorkerStopPoll::Completed(cleanup_authority_failure(
                    frontend_id,
                    kind,
                    generation,
                    error,
                )),
            },
        }
    }

    pub fn wait_until_finished(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> Result<bool, HalError> {
        match &self.kind {
            FrontendWorkerStopTicketKind::Immediate(_) => Ok(true),
            FrontendWorkerStopTicketKind::Retained { authority, .. } => {
                authority.inspect(|cleanup| match cleanup {
                    FrontendWorkerCleanup::Join(join) => join.wait_until_finished(deadline),
                    FrontendWorkerCleanup::BackendSubmitJoin(join) => {
                        join.wait_until_finished(deadline)
                    }
                    FrontendWorkerCleanup::Failed(_) => Ok(true),
                    FrontendWorkerCleanup::BackendSubmitPrepared { .. } => Ok(true),
                })?
            }
        }
    }
}

impl FrontendWorkerCancelReason {
    fn to_worker_stop_reason(self) -> WorkerStopReason {
        match self {
            FrontendWorkerCancelReason::StopRequested => WorkerStopReason::ExplicitClose,
            FrontendWorkerCancelReason::SupersededByNewRequest => WorkerStopReason::Reconfigure,
            FrontendWorkerCancelReason::FrontendClosing => WorkerStopReason::OwnerLoss,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrontendWorkerContext {
    frontend_id: i32,
    kind: FrontendWorkerKind,
    generation: u64,
    control: WorkerContext,
    cancel_reason: Arc<Mutex<Option<FrontendWorkerCancelReason>>>,
}

impl FrontendWorkerContext {
    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
    pub fn kind(&self) -> FrontendWorkerKind {
        self.kind
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn cancel_requested(&self) -> bool {
        self.control.stop_requested()
    }
    pub fn wait_until(&self, deadline: Option<std::time::Instant>) {
        self.control.wait_until(deadline)
    }
    pub fn cancel_reason(&self) -> Result<Option<FrontendWorkerCancelReason>, HalError> {
        self.cancel_reason.lock().map(|guard| *guard).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker cancel reason lock poisoned",
            )
        })
    }
}

#[derive(Debug)]
struct FrontendWorkerSlot {
    generation: u64,
    cancel_reason: Arc<Mutex<Option<FrontendWorkerCancelReason>>>,
    thread_result: Option<ThreadResultOwner<(Result<(), HalError>, WorkerExit)>>,
    pending_completed: Option<(Result<(), HalError>, WorkerExit)>,
}

impl FrontendWorkerSlot {
    fn request_stop_and_wake(&self) -> Result<(), HalError> {
        self.thread_result
            .as_ref()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker stop owner missing",
                )
            })?
            .request_stop();
        Ok(())
    }

    fn is_running(&mut self) -> bool {
        if self.pending_completed.is_some() {
            return false;
        }
        match self.completed_result() {
            Some(completed) => {
                self.pending_completed = Some(completed);
                false
            }
            None => true,
        }
    }

    fn completed_result(&mut self) -> Option<(Result<(), HalError>, WorkerExit)> {
        if let Some(completed) = self.pending_completed.take() {
            return Some(completed);
        }
        let owner = self.thread_result.as_mut()?;
        let terminal = owner.collect_terminal_if_finished()?;
        self.thread_result = None;
        Some(frontend_worker_terminal(terminal))
    }

    fn join_after_cancel(&mut self) -> (Result<(), HalError>, WorkerExit) {
        if let Some(completed) = self.pending_completed.take() {
            return completed;
        }
        let Some(owner) = self.thread_result.take() else {
            return (
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker slot missing thread result owner",
                )),
                WorkerExit::RuntimeFailure(WorkerFailureDomain::Backend.runtime_failure_kind()),
            );
        };
        frontend_worker_terminal(owner.join_terminal_after_stop())
    }
}

fn owner_failure_exit(error: &HalError) -> WorkerExit {
    match error {
        HalError::WorkerLockPoisoned { .. } => {
            WorkerExit::RuntimeFailure(WorkerFailureDomain::Signal.runtime_failure_kind())
        }
        _ => WorkerExit::RuntimeFailure(WorkerFailureDomain::Backend.runtime_failure_kind()),
    }
}

fn frontend_worker_terminal(
    terminal: WorkerTerminalResult<(Result<(), HalError>, WorkerExit)>,
) -> (Result<(), HalError>, WorkerExit) {
    match terminal {
        WorkerTerminalResult::Normal(completed) => completed,
        WorkerTerminalResult::StopRequested => (
            Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker terminal result is missing the domain stop reason",
            )),
            WorkerExit::RuntimeFailure(WorkerFailureDomain::Backend.runtime_failure_kind()),
        ),
        WorkerTerminalResult::RuntimeFailure(error) => {
            let exit = owner_failure_exit(&error);
            (Err(error), exit)
        }
        WorkerTerminalResult::PanicOrJoinFailure => (
            Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker panicked or could not be joined",
            )),
            WorkerExit::PanicOrJoinFailure,
        ),
    }
}

#[derive(Debug, Default)]
pub struct FrontendWorkerRegistry {
    slots: BTreeMap<FrontendWorkerKey, FrontendWorkerSlot>,
    cleanup: Vec<(
        FrontendWorkerKey,
        u64,
        WorkerRuntimeCleanup<FrontendWorkerCleanup>,
    )>,
}

impl FrontendWorkerRegistry {
    pub fn has_cleanup_obligations(&self) -> bool {
        !self.slots.is_empty() || self.cleanup.iter().any(|(_, _, owner)| owner.is_pending())
    }

    fn issue_cleanup(
        key: FrontendWorkerKey,
        generation: u64,
        reason: FrontendWorkerCancelReason,
        owner: &WorkerRuntimeCleanup<FrontendWorkerCleanup>,
    ) -> FrontendWorkerStopTicket {
        match owner.issue() {
            Ok(authority) => FrontendWorkerStopTicket {
                kind: FrontendWorkerStopTicketKind::Retained {
                    frontend_id: key.frontend_id,
                    kind: key.kind,
                    generation,
                    authority,
                },
            },
            Err(error) => {
                FrontendWorkerStopTicket::immediate(FrontendWorkerStopOutcome::StopRequestFailed {
                    frontend_id: key.frontend_id,
                    kind: key.kind,
                    generation,
                    reason,
                    error,
                })
            }
        }
    }

    pub fn prepare_backend_submit(
        &mut self,
        kind: FrontendWorkerKind,
        plan: FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    ) -> Result<FrontendWorkerStopTicket, HalError> {
        let (frontend_id, generation) = plan.worker_identity();
        self.cleanup.retain(|(_, _, owner)| owner.is_pending());
        let key = FrontendWorkerKey { frontend_id, kind };
        if self.cleanup.iter().any(|(pending, _, _)| *pending == key) {
            return Err(HalError::WorkerCleanupFailed {
                kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Executing,
            });
        }
        let owner = WorkerRuntime::retain_cleanup(FrontendWorkerCleanup::BackendSubmitPrepared {
            plan,
            previous_request,
        });
        let ticket = Self::issue_cleanup(
            key,
            generation,
            FrontendWorkerCancelReason::StopRequested,
            &owner,
        );
        self.cleanup.push((key, generation, owner));
        Ok(ticket)
    }

    #[cfg(test)]
    pub(super) fn retain_backend_submit_cleanup(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        ticket: FrontendBackendSubmitTicket,
    ) -> FrontendWorkerStopTicket {
        let key = FrontendWorkerKey { frontend_id, kind };
        let owner = WorkerRuntime::retain_cleanup(FrontendWorkerCleanup::BackendSubmitJoin(
            FrontendBackendSubmitDetachedJoin { frontend_id, kind, generation, ticket },
        ));
        let ticket = Self::issue_cleanup(key, generation, FrontendWorkerCancelReason::StopRequested, &owner);
        self.cleanup.push((key, generation, owner));
        ticket
    }

    pub fn start<F>(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        job: F,
    ) -> Result<(), FrontendWorkerStartError>
    where
        F: FnOnce(FrontendWorkerContext) -> Result<(), HalError> + Send + 'static,
    {
        let key = FrontendWorkerKey { frontend_id, kind };
        self.cleanup.retain(|(_, _, owner)| owner.is_pending());
        if let Some((_, generation, _)) =
            self.cleanup.iter().find(|(pending, _, _)| *pending == key)
        {
            return Err(FrontendWorkerStartError::AlreadyRunning {
                frontend_id,
                kind,
                generation: *generation,
            });
        }
        let mut remove_finished_success = false;
        if let Some(slot) = self.slots.get_mut(&key) {
            match slot.completed_result() {
                Some((Ok(()), _exit)) => {
                    // 正常終了済みworkerは置換できる。mutable borrowを解放してから
                    // 旧slotを削除する。
                    remove_finished_success = true;
                }
                Some((Err(error), exit)) => {
                    let generation = slot.generation;
                    slot.pending_completed = Some((Err(error.clone()), exit));
                    return Err(FrontendWorkerStartError::CompletedFailurePending {
                        frontend_id,
                        kind,
                        generation,
                        exit,
                        error,
                    });
                }
                None => {
                    return Err(FrontendWorkerStartError::AlreadyRunning {
                        frontend_id,
                        kind,
                        generation: slot.generation,
                    });
                }
            }
        }
        if remove_finished_success {
            self.slots.remove(&key);
        }

        let cancel_reason = Arc::new(Mutex::new(None));
        let worker_cancel_reason = Arc::clone(&cancel_reason);
        let thread_name: &'static str = "maleicacid-frontend-worker";
        let thread_result = ThreadResultOwner::start_controlled(thread_name, move |control| {
            let context = FrontendWorkerContext {
                frontend_id,
                kind,
                generation,
                control,
                cancel_reason: Arc::clone(&worker_cancel_reason),
            };
            match job(context) {
                Ok(()) => match worker_cancel_reason.lock() {
                    Ok(guard) => {
                        let exit = (*guard)
                            .map(|reason| WorkerExit::StopRequested(reason.to_worker_stop_reason()))
                            .unwrap_or(WorkerExit::Normal);
                        Ok((Ok(()), exit))
                    }
                    Err(_) => Ok((
                        Err(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend worker cancel reason lock poisoned",
                        )),
                        WorkerExit::RuntimeFailure(
                            WorkerFailureDomain::Signal.runtime_failure_kind(),
                        ),
                    )),
                },
                Err(error) => Ok((
                    Err(error),
                    WorkerExit::RuntimeFailure(WorkerFailureDomain::Backend.runtime_failure_kind()),
                )),
            }
        })
        .map_err(|error| FrontendWorkerStartError::SpawnFailed { error })?;

        self.slots.insert(
            key,
            FrontendWorkerSlot {
                generation,
                cancel_reason,
                thread_result: Some(thread_result),
                pending_completed: None,
            },
        );
        Ok(())
    }

    pub fn request_stop(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        let key = FrontendWorkerKey { frontend_id, kind };
        let Some(slot) = self.slots.get_mut(&key) else {
            return FrontendWorkerStopOutcome::NotRunning;
        };
        if let Some((result, exit)) = slot.completed_result() {
            let generation = slot.generation;
            self.slots.remove(&key);
            return FrontendWorkerStopOutcome::Completed {
                frontend_id,
                kind,
                generation,
                exit,
                result,
            };
        }
        let generation = slot.generation;
        let cancel_reason = Arc::clone(&slot.cancel_reason);
        let Ok(mut guard) = cancel_reason.lock() else {
            return FrontendWorkerStopOutcome::StopRequestFailed {
                frontend_id,
                kind,
                generation,
                reason,
                error: HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker cancel reason lock poisoned",
                ),
            };
        };
        *guard = Some(reason);
        drop(guard);
        if let Err(error) = slot.request_stop_and_wake() {
            return FrontendWorkerStopOutcome::StopRequestFailed {
                frontend_id,
                kind,
                generation,
                reason,
                error,
            };
        }
        FrontendWorkerStopOutcome::CancelRequested {
            frontend_id,
            kind,
            generation: slot.generation,
            reason,
        }
    }

    pub fn request_stop_for_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopTicket {
        let key = FrontendWorkerKey { frontend_id, kind };
        self.cleanup.retain(|(_, _, owner)| owner.is_pending());
        if let Some((_, generation, owner)) =
            self.cleanup.iter().find(|(pending, _, _)| *pending == key)
        {
            return Self::issue_cleanup(key, *generation, reason, owner);
        }
        let Some(mut slot) = self.slots.remove(&key) else {
            return FrontendWorkerStopTicket::immediate(FrontendWorkerStopOutcome::NotRunning);
        };

        if let Some(completed) = slot.completed_result() {
            slot.pending_completed = Some(completed);
        }

        let generation = slot.generation;
        if slot.pending_completed.is_none() {
            let cancel_reason = Arc::clone(&slot.cancel_reason);
            let mut guard = match cancel_reason.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    self.slots.insert(key, slot);
                    return FrontendWorkerStopTicket::immediate(
                        FrontendWorkerStopOutcome::StopRequestFailed {
                            frontend_id,
                            kind,
                            generation,
                            reason,
                            error: HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend worker cancel reason lock poisoned",
                            ),
                        },
                    );
                }
            };
            *guard = Some(reason);
            drop(guard);
            if let Err(error) = slot.request_stop_and_wake() {
                self.slots.insert(key, slot);
                return FrontendWorkerStopTicket::immediate(
                    FrontendWorkerStopOutcome::StopRequestFailed {
                        frontend_id,
                        kind,
                        generation,
                        reason,
                        error,
                    },
                );
            }
        }
        let owner = WorkerRuntime::retain_cleanup(FrontendWorkerCleanup::Join(
            FrontendWorkerDetachedJoin {
                frontend_id,
                kind,
                generation,
                slot,
            },
        ));
        let ticket = Self::issue_cleanup(key, generation, reason, &owner);
        self.cleanup.push((key, generation, owner));
        ticket
    }

    pub fn take_completed(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Option<FrontendWorkerStopOutcome> {
        let key = FrontendWorkerKey { frontend_id, kind };
        let slot = self.slots.get_mut(&key)?;
        let (result, exit) = slot.completed_result()?;
        let generation = slot.generation;
        self.slots.remove(&key);
        Some(FrontendWorkerStopOutcome::Completed {
            frontend_id,
            kind,
            generation,
            exit,
            result,
        })
    }

    pub fn running_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Option<u64> {
        let key = FrontendWorkerKey { frontend_id, kind };
        let slot = self.slots.get_mut(&key)?;
        slot.is_running().then_some(slot.generation)
    }

    pub fn clear_finished(&mut self) {
        let keys: Vec<_> = self
            .slots
            .iter_mut()
            .filter_map(|(key, slot)| match slot.completed_result() {
                Some((Ok(()), _exit)) => Some(*key),
                Some(completed) => {
                    slot.pending_completed = Some(completed);
                    None
                }
                None => None,
            })
            .collect();
        for key in keys {
            self.slots.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    fn submit_plan() -> FrontendBackendTunePlan {
        use maleicacid_tuner_hal2_common::{
            FrontendBackendKind, FrontendDevicePath, FrontendIsdbtPartialReceptionRequirement,
            FrontendSystem,
        };
        FrontendBackendTunePlan::new(7, 1, FrontendBackendKind::LinuxDvb,
            FrontendDevicePath::new("/unused-submit-test"), FrontendTuneRequest {
                system: FrontendSystem::IsdbT,
                frequency: 473_142_857,
                end_frequency: None,
                stream_id: None,
                stream_id_kind: None,
                bandwidth_hz: Some(6_000_000),
                symbol_rate: None,
                isdbt_layer_settings: Vec::new(),
                partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
            })
    }

    #[test]
    fn abandoned_prepared_submit_keeps_obligation_and_can_be_cancelled() {
        let mut registry = FrontendWorkerRegistry::default();
        let ticket = registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).unwrap();
        std::mem::forget(ticket);
        assert!(registry.has_cleanup_obligations());
        assert!(registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).is_err());
        let cancellation = registry.request_stop_for_join(7, FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested);
        assert_eq!(cancellation.complete(), FrontendWorkerStopOutcome::NotRunning);
        assert!(!registry.has_cleanup_obligations());
    }

    #[test]
    fn superseded_submit_authority_does_not_start_external_work() {
        let mut registry = FrontendWorkerRegistry::default();
        let ticket = registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).unwrap();
        let cancellation = registry.request_stop_for_join(7, FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested);
        assert!(ticket.submit_until_with(std::time::Instant::now(), |_, _| panic!("must not start")).is_err());
        assert_eq!(cancellation.complete(), FrontendWorkerStopOutcome::NotRunning);
    }

    #[test]
    fn interrupted_submit_retains_indeterminate_obligation_and_blocks_reuse() {
        let mut registry = FrontendWorkerRegistry::default();
        let ticket = registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).unwrap();
        assert!(matches!(ticket.submit_until_with(std::time::Instant::now(), |_, _| panic!("injected interruption")),
            Err(HalError::WorkerCleanupFailed { kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Interrupted })));
        assert!(registry.has_cleanup_obligations());
        assert!(registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).is_err());
        assert!(matches!(registry.request_stop_for_join(7, FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested).complete(),
            FrontendWorkerStopOutcome::StopRequestFailed { error: HalError::WorkerCleanupFailed {
                kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Quarantined }, .. }));
    }

    #[test]
    fn submit_spawn_failure_completes_the_unstarted_obligation() {
        let mut registry = FrontendWorkerRegistry::default();
        let ticket = registry.prepare_backend_submit(FrontendWorkerKind::Tune, submit_plan(), None).unwrap();
        let result = ticket.submit_until_with(std::time::Instant::now(), |_, _|
            Err(HalError::internal(HalInternalKind::InvariantViolation, "spawn failed"))).unwrap();
        assert!(matches!(result, FrontendWorkerSubmitWait::Completed(Err(FrontendBackendSubmitFailure {
            rollback_succeeded: true, .. }))));
        assert!(!registry.has_cleanup_obligations());
    }

    #[test]
    fn duplicate_running_worker_is_rejected() {
        let mut registry = FrontendWorkerRegistry::default();
        let (tx, rx) = mpsc::channel();
        registry
            .start(7, FrontendWorkerKind::Tune, 1, move |ctx| {
                tx.send(ctx.generation()).unwrap();
                while !ctx.cancel_requested() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert!(matches!(
            registry.start(7, FrontendWorkerKind::Tune, 2, |_| Ok(())),
            Err(FrontendWorkerStartError::AlreadyRunning { generation: 1, .. })
        ));
        assert!(matches!(
            registry.request_stop(
                7,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested
            ),
            FrontendWorkerStopOutcome::CancelRequested {
                generation: 1,
                reason: FrontendWorkerCancelReason::StopRequested,
                ..
            }
        ));
        for _ in 0..100 {
            if registry
                .take_completed(7, FrontendWorkerKind::Tune)
                .is_some()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("cancelled worker did not complete");
    }

    #[test]
    fn cancellation_reason_is_visible_to_worker() {
        let mut registry = FrontendWorkerRegistry::default();
        let (started_tx, started_rx) = mpsc::channel();
        let (reason_tx, reason_rx) = mpsc::channel();
        registry
            .start(10, FrontendWorkerKind::Scan, 5, move |ctx| {
                started_tx.send(()).unwrap();
                while !ctx.cancel_requested() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                reason_tx.send(ctx.cancel_reason().unwrap()).unwrap();
                Ok(())
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            registry.request_stop(
                10,
                FrontendWorkerKind::Scan,
                FrontendWorkerCancelReason::SupersededByNewRequest
            ),
            FrontendWorkerStopOutcome::CancelRequested {
                reason: FrontendWorkerCancelReason::SupersededByNewRequest,
                ..
            }
        ));
        assert_eq!(
            reason_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Some(FrontendWorkerCancelReason::SupersededByNewRequest)
        );
        for _ in 0..100 {
            if registry
                .take_completed(10, FrontendWorkerKind::Scan)
                .is_some()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("cancelled worker did not complete");
    }

    #[test]
    fn completed_worker_result_is_reported_and_slot_removed() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(8, FrontendWorkerKind::Scan, 3, |_| Ok(()))
            .unwrap();
        for _ in 0..100 {
            if registry
                .take_completed(8, FrontendWorkerKind::Scan)
                .is_some()
            {
                assert!(registry
                    .running_generation(8, FrontendWorkerKind::Scan)
                    .is_none());
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("worker did not complete");
    }
    #[test]
    fn failed_worker_is_reported_as_error_and_slot_removed() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(
                9,
                FrontendWorkerKind::Tune,
                4,
                |_| -> Result<(), HalError> {
                    Err(HalError::cleanup_failed(
                        "frontend worker test",
                        "forced failure",
                    ))
                },
            )
            .unwrap();
        for _ in 0..100 {
            if let Some(FrontendWorkerStopOutcome::Completed { result, .. }) =
                registry.take_completed(9, FrontendWorkerKind::Tune)
            {
                assert!(result.is_err());
                assert!(registry
                    .running_generation(9, FrontendWorkerKind::Tune)
                    .is_none());
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("panicked worker was not reported");
    }

    #[test]
    fn stop_and_join_removes_running_worker_and_allows_replacement() {
        let mut registry = FrontendWorkerRegistry::default();
        let (started_tx, started_rx) = mpsc::channel();
        registry
            .start(12, FrontendWorkerKind::Scan, 8, move |ctx| {
                started_tx.send(()).unwrap();
                while !ctx.cancel_requested() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(
                    ctx.cancel_reason().unwrap(),
                    Some(FrontendWorkerCancelReason::SupersededByNewRequest)
                );
                Ok(())
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            registry
                .request_stop_for_join(
                    12,
                    FrontendWorkerKind::Scan,
                    FrontendWorkerCancelReason::SupersededByNewRequest
                )
                .complete(),
            FrontendWorkerStopOutcome::Completed {
                generation: 8,
                result: Ok(()),
                ..
            }
        ));
        assert!(matches!(
            registry.start(12, FrontendWorkerKind::Scan, 9, |_| Ok(())),
            Ok(())
        ));
    }

    #[test]
    fn clear_finished_keeps_failed_worker_for_reporting() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(
                13,
                FrontendWorkerKind::Tune,
                9,
                |_| -> Result<(), HalError> {
                    Err(HalError::cleanup_failed(
                        "frontend worker test",
                        "forced failure",
                    ))
                },
            )
            .unwrap();
        for _ in 0..100 {
            registry.clear_finished();
            if let Some(FrontendWorkerStopOutcome::Completed { result, .. }) =
                registry.take_completed(13, FrontendWorkerKind::Tune)
            {
                assert!(result.is_err());
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("failed worker was removed or not reported");
    }

    #[test]
    fn missing_worker_result_is_not_converted_to_success() {
        let mut registry = FrontendWorkerRegistry::default();
        let key = FrontendWorkerKey {
            frontend_id: 14,
            kind: FrontendWorkerKind::Tune,
        };
        registry.slots.insert(
            key,
            FrontendWorkerSlot {
                generation: 10,
                cancel_reason: Arc::new(Mutex::new(None)),
                thread_result: Some(
                    ThreadResultOwner::start(
                        "frontend-worker-owner-failure-test",
                        || -> Result<(Result<(), HalError>, WorkerExit), HalError> {
                            panic!("forced worker owner failure")
                        },
                    )
                    .unwrap(),
                ),
                pending_completed: None,
            },
        );

        match registry
            .request_stop_for_join(
                14,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested,
            )
            .complete()
        {
            FrontendWorkerStopOutcome::Completed { result, exit, .. } => {
                assert!(result.is_err());
                assert_eq!(exit, WorkerExit::PanicOrJoinFailure);
            }
            other => panic!("unexpected stop outcome: {other:?}"),
        }
    }

    #[test]
    fn start_does_not_overwrite_unreported_worker_failure() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(
                16,
                FrontendWorkerKind::Tune,
                12,
                |_| -> Result<(), HalError> {
                    Err(HalError::cleanup_failed(
                        "frontend worker test",
                        "pending failure",
                    ))
                },
            )
            .unwrap();
        for _ in 0..100 {
            if matches!(
                registry.start(16, FrontendWorkerKind::Tune, 13, |_| Ok(())),
                Err(FrontendWorkerStartError::CompletedFailurePending { generation: 12, .. })
            ) {
                match registry.take_completed(16, FrontendWorkerKind::Tune) {
                    Some(FrontendWorkerStopOutcome::Completed { result, .. }) => {
                        assert!(result.is_err());
                        return;
                    }
                    other => panic!("pending failure was not preserved: {other:?}"),
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("pending worker failure was not observed");
    }

    #[test]
    fn replacement_keeps_pending_failure_and_exit_typed() {
        let mut registry = FrontendWorkerRegistry::default();
        let error = HalError::WorkerLockPoisoned {
            owner: "frontend",
            lock: maleicacid_tuner_hal2_common::WorkerLockKind::Completion,
        };
        let exit = owner_failure_exit(&error);
        registry.slots.insert(
            FrontendWorkerKey {
                frontend_id: 16,
                kind: FrontendWorkerKind::Tune,
            },
            FrontendWorkerSlot {
                generation: 12,
                cancel_reason: Arc::new(Mutex::new(None)),
                thread_result: None,
                pending_completed: Some((Err(error.clone()), exit)),
            },
        );
        assert_eq!(
            registry.start(16, FrontendWorkerKind::Tune, 13, |_| Ok(())),
            Err(FrontendWorkerStartError::CompletedFailurePending {
                frontend_id: 16,
                kind: FrontendWorkerKind::Tune,
                generation: 12,
                exit,
                error: error.clone(),
            })
        );
        assert_eq!(
            registry.take_completed(16, FrontendWorkerKind::Tune),
            Some(FrontendWorkerStopOutcome::Completed {
                frontend_id: 16,
                kind: FrontendWorkerKind::Tune,
                generation: 12,
                exit,
                result: Err(error),
            })
        );
    }

    #[test]
    fn failed_join_keeps_the_obligation_and_blocks_replacement() {
        for poll in [false, true] {
            let mut registry = FrontendWorkerRegistry::default();
            let key = FrontendWorkerKey {
                frontend_id: 16,
                kind: FrontendWorkerKind::Tune,
            };
            let error = HalError::cleanup_failed("backend stop", "device still active");
            registry.slots.insert(
                key,
                FrontendWorkerSlot {
                    generation: 12,
                    cancel_reason: Arc::new(Mutex::new(None)),
                    thread_result: None,
                    pending_completed: Some((Err(error.clone()), WorkerExit::Normal)),
                },
            );
            let ticket = registry.request_stop_for_join(
                16,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested,
            );
            let outcome = if poll {
                match ticket.try_complete() {
                    FrontendWorkerStopPoll::Completed(outcome) => outcome,
                    _ => panic!("already completed worker remained pending"),
                }
            } else {
                ticket.complete()
            };
            assert!(
                matches!(outcome, FrontendWorkerStopOutcome::Completed { result: Err(ref recorded), .. } if recorded == &error)
            );
            assert!(registry.has_cleanup_obligations());
            assert!(matches!(
                registry.start(16, FrontendWorkerKind::Tune, 13, |_| Ok(())),
                Err(FrontendWorkerStartError::AlreadyRunning { generation: 12, .. })
            ));
            assert!(matches!(
                registry
                    .request_stop_for_join(
                        16,
                        FrontendWorkerKind::Tune,
                        FrontendWorkerCancelReason::StopRequested
                    )
                    .complete(),
                FrontendWorkerStopOutcome::StopRequestFailed {
                    error: HalError::WorkerCleanupFailed {
                        kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Quarantined
                    },
                    ..
                }
            ));
        }
    }

    #[test]
    fn lost_stop_ticket_can_be_reissued_and_joined_without_replacing_the_worker() {
        for forget in [false, true] {
            let mut registry = FrontendWorkerRegistry::default();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            registry
                .start(21, FrontendWorkerKind::Tune, 5, move |_| {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
            started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            let first = registry.request_stop_for_join(
                21,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested,
            );
            if forget {
                std::mem::forget(first);
            } else {
                drop(first);
            }
            assert!(registry.has_cleanup_obligations());
            assert!(matches!(
                registry.start(21, FrontendWorkerKind::Tune, 6, |_| Ok(())),
                Err(FrontendWorkerStartError::AlreadyRunning { generation: 5, .. })
            ));
            let next = registry.request_stop_for_join(
                21,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested,
            );
            release_tx.send(()).unwrap();
            assert!(matches!(
                next.complete(),
                FrontendWorkerStopOutcome::Completed {
                    generation: 5,
                    result: Ok(()),
                    exit: WorkerExit::StopRequested(WorkerStopReason::ExplicitClose),
                    ..
                }
            ));
            assert!(!registry.has_cleanup_obligations());
        }
    }
}
