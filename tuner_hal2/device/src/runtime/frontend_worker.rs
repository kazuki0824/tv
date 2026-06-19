//! frontend 非同期worker slot所有。
//!
//! このmoduleは並行処理境界だけを所有する。tune/scan成功を装わず、呼び出し元がbackend jobを渡し、slotは完了・取消状態だけを記録する。
//! worker slotは完了・取消・失敗状態だけを保持し、実operationの成功を代用しない。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_control_core::{WorkerExit, WorkerFailureDomain, WorkerStopReason};

use crate::runtime::thread_result_owner::{ThreadResultOwner, ThreadResultPoll};

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
        detail: String,
    },
    SpawnFailed {
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendWorkerStopOutcome {
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


#[derive(Debug)]
pub struct FrontendWorkerDetachedJoin {
    frontend_id: i32,
    kind: FrontendWorkerKind,
    generation: u64,
    slot: FrontendWorkerSlot,
}

impl FrontendWorkerDetachedJoin {
    pub fn complete(self) -> FrontendWorkerStopOutcome {
        let (result, exit) = self.slot.join_after_cancel();
        FrontendWorkerStopOutcome::Completed {
            frontend_id: self.frontend_id,
            kind: self.kind,
            generation: self.generation,
            exit,
            result,
        }
    }
}

#[derive(Debug)]
pub enum FrontendWorkerStopTicket {
    Immediate(FrontendWorkerStopOutcome),
    Join(FrontendWorkerDetachedJoin),
}

impl FrontendWorkerStopTicket {
    pub fn complete(self) -> FrontendWorkerStopOutcome {
        match self {
            FrontendWorkerStopTicket::Immediate(outcome) => outcome,
            FrontendWorkerStopTicket::Join(join) => join.complete(),
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
    cancel: Arc<AtomicBool>,
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
        self.cancel.load(Ordering::SeqCst)
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
    cancel: Arc<AtomicBool>,
    cancel_reason: Arc<Mutex<Option<FrontendWorkerCancelReason>>>,
    thread_result: Option<ThreadResultOwner<(Result<(), HalError>, WorkerExit)>>,
    pending_completed: Option<(Result<(), HalError>, WorkerExit)>,
}

impl FrontendWorkerSlot {
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
        match owner.collect_if_finished() {
            ThreadResultPoll::Running => None,
            ThreadResultPoll::Completed(Ok(completed)) => {
                self.thread_result = None;
                Some(completed)
            }
            ThreadResultPoll::Completed(Err(error)) => {
                self.thread_result = None;
                Some((Err(error), WorkerExit::PanicOrJoinFailure))
            }
        }
    }

    fn join_after_cancel(mut self) -> (Result<(), HalError>, WorkerExit) {
        if let Some(completed) = self.pending_completed.take() {
            return completed;
        }
        let Some(owner) = self.thread_result.take() else {
            return (
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker slot missing thread result owner",
                )),
                WorkerExit::PanicOrJoinFailure,
            );
        };
        match owner.join_after_stop() {
            Ok(completed) => completed,
            Err(error) => (Err(error), WorkerExit::PanicOrJoinFailure),
        }
    }
}

#[derive(Debug, Default)]
pub struct FrontendWorkerRegistry {
    slots: BTreeMap<FrontendWorkerKey, FrontendWorkerSlot>,
}

impl FrontendWorkerRegistry {
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
        let mut remove_finished_success = false;
        if let Some(slot) = self.slots.get_mut(&key) {
            match slot.completed_result() {
                Some((Ok(()), _exit)) => {
                    // A successfully completed worker may be replaced. Remove the
                    // old slot after releasing the mutable slot borrow.
                    remove_finished_success = true;
                }
                Some((Err(error), exit)) => {
                    let generation = slot.generation;
                    let detail = format!("{error:?}");
                    slot.pending_completed = Some((Err(error), exit));
                    return Err(FrontendWorkerStartError::CompletedFailurePending {
                        frontend_id,
                        kind,
                        generation,
                        detail,
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

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_reason = Arc::new(Mutex::new(None));
        let worker_cancel = Arc::clone(&cancel);
        let worker_cancel_reason = Arc::clone(&cancel_reason);
        let context = FrontendWorkerContext {
            frontend_id,
            kind,
            generation,
            cancel: worker_cancel,
            cancel_reason: Arc::clone(&worker_cancel_reason),
        };

        let thread_name: &'static str = "maleicacid-frontend-worker";
        let thread_result = ThreadResultOwner::start(thread_name, move || {
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
                    WorkerExit::RuntimeFailure(
                        WorkerFailureDomain::Backend.runtime_failure_kind(),
                    ),
                )),
            }
        })
        .map_err(|error| FrontendWorkerStartError::SpawnFailed {
            detail: format!("{error:?}"),
        })?;

        self.slots.insert(
            key,
            FrontendWorkerSlot {
                generation,
                cancel,
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
        slot.cancel.store(true, Ordering::SeqCst);
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
        let Some(mut slot) = self.slots.remove(&key) else {
            return FrontendWorkerStopTicket::Immediate(FrontendWorkerStopOutcome::NotRunning);
        };

        if let Some((result, exit)) = slot.completed_result() {
            return FrontendWorkerStopTicket::Immediate(FrontendWorkerStopOutcome::Completed {
                frontend_id,
                kind,
                generation: slot.generation,
                exit,
                result,
            });
        }

        let generation = slot.generation;
        let cancel_reason = Arc::clone(&slot.cancel_reason);
        let mut guard = match cancel_reason.lock() {
            Ok(guard) => guard,
            Err(_) => {
                self.slots.insert(key, slot);
                return FrontendWorkerStopTicket::Immediate(
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
        slot.cancel.store(true, Ordering::SeqCst);

        FrontendWorkerStopTicket::Join(FrontendWorkerDetachedJoin {
            frontend_id,
            kind,
            generation,
            slot,
        })
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
    fn panicked_worker_is_reported_as_error_and_slot_removed() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(
                9,
                FrontendWorkerKind::Tune,
                4,
                |_| -> Result<(), HalError> {
                    panic!("intentional test panic");
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
    fn request_stop_reports_cancel_reason_lock_poison() {
        let mut registry = FrontendWorkerRegistry::default();
        let (started_tx, started_rx) = mpsc::channel();
        registry
            .start(11, FrontendWorkerKind::Tune, 6, move |ctx| {
                started_tx.send(()).unwrap();
                while !ctx.cancel_requested() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let key = FrontendWorkerKey {
            frontend_id: 11,
            kind: FrontendWorkerKind::Tune,
        };
        let cancel_reason = registry
            .slots
            .get(&key)
            .expect("worker slot must exist")
            .cancel_reason
            .clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = cancel_reason.lock().unwrap();
            panic!("poison cancel reason lock");
        }));
        assert!(matches!(
            registry.request_stop(
                11,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested
            ),
            FrontendWorkerStopOutcome::StopRequestFailed {
                generation: 6,
                ..
            }
        ));
        if let Some(mut slot) = registry.slots.remove(&key) {
            slot.cancel.store(true, Ordering::SeqCst);
            let _ = slot.join_after_cancel();
        }
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
            registry.request_stop_for_join(
                12,
                FrontendWorkerKind::Scan,
                FrontendWorkerCancelReason::SupersededByNewRequest
            ).complete(),
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
            .start(13, FrontendWorkerKind::Tune, 9, |_| -> Result<(), HalError> {
                panic!("intentional test panic");
            })
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
        let result: Arc<Mutex<Option<Result<(Result<(), HalError>, WorkerExit), HalError>>>> =
            Arc::new(Mutex::new(None));
        let join = std::thread::spawn(|| {});
        registry.slots.insert(
            key,
            FrontendWorkerSlot {
                generation: 10,
                cancel: Arc::new(AtomicBool::new(false)),
                cancel_reason: Arc::new(Mutex::new(None)),
                thread_result: Some(ThreadResultOwner::new_for_test(
                    "frontend-worker-missing-test",
                    result,
                    Some(join),
                )),
                pending_completed: None,
            },
        );

        match registry.request_stop_for_join(
            14,
            FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested,
        ).complete() {
            FrontendWorkerStopOutcome::Completed { result, exit, .. } => {
                assert!(result.is_err());
                assert_eq!(exit, WorkerExit::PanicOrJoinFailure);
            }
            other => panic!("unexpected stop outcome: {other:?}"),
        }
    }

    #[test]
    fn clear_finished_does_not_silently_remove_poison_result_slot() {
        let mut registry = FrontendWorkerRegistry::default();
        let key = FrontendWorkerKey {
            frontend_id: 15,
            kind: FrontendWorkerKind::Scan,
        };
        let result: Arc<Mutex<Option<Result<(Result<(), HalError>, WorkerExit), HalError>>>> =
            Arc::new(Mutex::new(None));
        let poisoned = Arc::clone(&result);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned.lock().unwrap();
            panic!("poison frontend worker result");
        }));
        let join = std::thread::spawn(|| {});
        registry.slots.insert(
            key,
            FrontendWorkerSlot {
                generation: 11,
                cancel: Arc::new(AtomicBool::new(false)),
                cancel_reason: Arc::new(Mutex::new(None)),
                thread_result: Some(ThreadResultOwner::new_for_test(
                    "frontend-worker-poison-test",
                    result,
                    Some(join),
                )),
                pending_completed: None,
            },
        );

        registry.clear_finished();
        match registry.take_completed(15, FrontendWorkerKind::Scan) {
            Some(FrontendWorkerStopOutcome::Completed { result, exit, .. }) => {
                assert!(result.is_err());
                assert_eq!(exit, WorkerExit::PanicOrJoinFailure);
            }
            other => panic!("poison result slot was silently removed: {other:?}"),
        }
    }

    #[test]
    fn start_does_not_overwrite_unreported_worker_failure() {
        let mut registry = FrontendWorkerRegistry::default();
        registry
            .start(16, FrontendWorkerKind::Tune, 12, |_| -> Result<(), HalError> {
                panic!("intentional pending failure");
            })
            .unwrap();
        for _ in 0..100 {
            if matches!(
                registry.start(16, FrontendWorkerKind::Tune, 13, |_| Ok(())),
                Err(FrontendWorkerStartError::CompletedFailurePending {
                    generation: 12,
                    ..
                })
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

}
