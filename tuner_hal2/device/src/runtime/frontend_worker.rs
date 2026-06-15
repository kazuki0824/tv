//! frontend 非同期worker slot所有。
//!
//! このmoduleは並行処理境界だけを所有する。tune/scan成功を装わず、呼び出し元がbackend jobを渡し、slotは完了・取消状態だけを記録する。
//! worker slotは完了・取消・失敗状態だけを保持し、実operationの成功を代用しない。

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_control_core::{WorkerExit, WorkerFailureDomain, WorkerStopReason};

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
    result: Arc<Mutex<Option<(Result<(), HalError>, WorkerExit)>>>,
    join: Option<JoinHandle<()>>,
    join_failure: Option<HalError>,
}

impl FrontendWorkerSlot {
    fn is_running(&self) -> bool {
        let completed = self
            .result
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(true);
        !completed
            && self
                .join
                .as_ref()
                .map(|handle| !handle.is_finished())
                .unwrap_or(false)
    }

    fn join_if_finished(&mut self) {
        if self
            .join
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(false)
        {
            if let Some(handle) = self.join.take() {
                match handle.join() {
                    Ok(()) => {}
                    Err(_) => {
                        self.join_failure = Some(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend worker thread panicked",
                        ));
                    }
                }
            }
        }
    }

    fn completed_result(&mut self) -> Option<(Result<(), HalError>, WorkerExit)> {
        self.join_if_finished();
        if let Some(error) = self.join_failure.take() {
            return Some((Err(error), WorkerExit::PanicOrJoinFailure));
        }
        match self.result.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => Some((
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker result lock poisoned",
                )),
                WorkerExit::RuntimeFailure(WorkerFailureDomain::Signal.runtime_failure_kind()),
            )),
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
        if let Some(slot) = self.slots.get_mut(&key) {
            if slot.is_running() {
                return Err(FrontendWorkerStartError::AlreadyRunning {
                    frontend_id,
                    kind,
                    generation: slot.generation,
                });
            }
            slot.join_if_finished();
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_reason = Arc::new(Mutex::new(None));
        let result = Arc::new(Mutex::new(None));
        let worker_cancel = Arc::clone(&cancel);
        let worker_cancel_reason = Arc::clone(&cancel_reason);
        let worker_result = Arc::clone(&result);
        let context = FrontendWorkerContext {
            frontend_id,
            kind,
            generation,
            cancel: worker_cancel,
            cancel_reason: Arc::clone(&worker_cancel_reason),
        };

        let join = thread::Builder::new()
            .name(format!("maleicacid-fe-{frontend_id}-{kind:?}-{generation}"))
            .spawn(move || {
                let outcome = match catch_unwind(AssertUnwindSafe(|| job(context))) {
                    Ok(Ok(())) => match worker_cancel_reason.lock() {
                        Ok(guard) => {
                            let exit = (*guard)
                                .map(|reason| WorkerExit::StopRequested(reason.to_worker_stop_reason()))
                                .unwrap_or(WorkerExit::Normal);
                            (Ok(()), exit)
                        }
                        Err(_) => (
                            Err(HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend worker cancel reason lock poisoned",
                            )),
                            WorkerExit::RuntimeFailure(
                                WorkerFailureDomain::Signal.runtime_failure_kind(),
                            ),
                        ),
                    }
                    Ok(Err(error)) => (
                        Err(error),
                        WorkerExit::RuntimeFailure(
                            WorkerFailureDomain::Backend.runtime_failure_kind(),
                        ),
                    ),
                    Err(_) => (
                        Err(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend worker thread panicked",
                        )),
                        WorkerExit::PanicOrJoinFailure,
                    ),
                };
                if let Ok(mut guard) = worker_result.lock() {
                    *guard = Some(outcome);
                }
            })
            .map_err(|error| FrontendWorkerStartError::SpawnFailed {
                detail: error.to_string(),
            })?;

        self.slots.insert(
            key,
            FrontendWorkerSlot {
                generation,
                cancel,
                cancel_reason,
                result,
                join: Some(join),
                join_failure: None,
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

    pub fn request_stop_and_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        let key = FrontendWorkerKey { frontend_id, kind };
        let Some(mut slot) = self.slots.remove(&key) else {
            return FrontendWorkerStopOutcome::NotRunning;
        };

        if let Some((result, exit)) = slot.completed_result() {
            return FrontendWorkerStopOutcome::Completed {
                frontend_id,
                kind,
                generation: slot.generation,
                exit,
                result,
            };
        }

        let generation = slot.generation;
        let cancel_reason = Arc::clone(&slot.cancel_reason);
        let mut guard = match cancel_reason.lock() {
            Ok(guard) => guard,
            Err(_) => {
                self.slots.insert(key, slot);
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
            }
        };
        *guard = Some(reason);
        drop(guard);
        slot.cancel.store(true, Ordering::SeqCst);

        if let Some(handle) = slot.join.take() {
            if handle.join().is_err() {
                return FrontendWorkerStopOutcome::Completed {
                    frontend_id,
                    kind,
                    generation: slot.generation,
                    exit: WorkerExit::PanicOrJoinFailure,
                    result: Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker thread panicked while stopping",
                    )),
                };
            }
        }

        let (result, exit) = match slot.result.lock() {
            Ok(mut guard) => guard.take().unwrap_or_else(|| {
                (
                    Ok(()),
                    WorkerExit::StopRequested(reason.to_worker_stop_reason()),
                )
            }),
            Err(_) => (
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker result lock poisoned while stopping",
                )),
                WorkerExit::RuntimeFailure(WorkerFailureDomain::Signal.runtime_failure_kind()),
            ),
        };
        FrontendWorkerStopOutcome::Completed {
            frontend_id,
            kind,
            generation: slot.generation,
            exit,
            result,
        }
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
            .filter_map(|(key, slot)| {
                slot.join_if_finished();
                let finished = slot.join_failure.is_some()
                    || slot
                        .result
                        .lock()
                        .map(|guard| guard.is_some())
                        .unwrap_or(true);
                finished.then_some(*key)
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
    use std::sync::mpsc;
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
            if let Some(handle) = slot.join.take() {
                handle.join().unwrap();
            }
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
            registry.request_stop_and_join(
                12,
                FrontendWorkerKind::Scan,
                FrontendWorkerCancelReason::SupersededByNewRequest
            ),
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
}
