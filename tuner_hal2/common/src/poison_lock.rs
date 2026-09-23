use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeLockKind {
    CallbackStore,
    CallbackDeathGate,
    CallbackDeathRecipient,
    FilterCallbackFallbackDiagnostics,
    FrontendCallbackFallbackDiagnostics,
    DropLeakDiagnostics,
    CleanupReaperOwner,
    DvrPostCommitDiagnostics,
    DvrNotifierCleanupDiagnostics,
    CallbackRuntimeSplitDiagnostics,
    FrontendCancelReason,
    SupervisorState,
    DvrQueueEpoch { queue_identity: Option<u64> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockPoisonDiagnostic {
    pub lock: RuntimeLockKind,
    pub poison_count: u64,
    pub counter_saturated: bool,
}

/// 保護データを復旧せず、汚染検出回数をロックの寿命内で保持する。
#[derive(Debug)]
pub struct PoisonTrackedMutex<T> {
    inner: Mutex<T>,
    kind: RuntimeLockKind,
    poison_count: AtomicU64,
}

impl<T> PoisonTrackedMutex<T> {
    pub fn new(value: T, kind: RuntimeLockKind) -> Self {
        Self { inner: Mutex::new(value), kind, poison_count: AtomicU64::new(0) }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T>, LockPoisonDiagnostic> {
        self.inner.lock().map_err(|error| {
            drop(error);
            self.record_poison()
        })
    }

    pub fn wait<'a>(&self, condition: &Condvar, guard: MutexGuard<'a, T>) -> Result<MutexGuard<'a, T>, LockPoisonDiagnostic> {
        condition.wait(guard).map_err(|error| {
            drop(error);
            self.record_poison()
        })
    }

    fn record_poison(&self) -> LockPoisonDiagnostic {
        let previous = self.poison_count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| Some(count.saturating_add(1))).unwrap_or_else(|count| count);
        let poison_count = previous.saturating_add(1);
        LockPoisonDiagnostic { lock: self.kind, poison_count, counter_saturated: poison_count == u64::MAX }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn poisoned_condition_reacquisition_keeps_identity() {
        let lock = std::sync::Arc::new(PoisonTrackedMutex::new((), RuntimeLockKind::DvrQueueEpoch { queue_identity: Some(7) }));
        let condition = std::sync::Arc::new(Condvar::new());
        let guard = lock.lock().unwrap();
        let other_lock = std::sync::Arc::clone(&lock);
        let other_condition = std::sync::Arc::clone(&condition);
        let worker = std::thread::spawn(move || {
            let _ = std::panic::catch_unwind(|| {
                let _guard = other_lock.lock().unwrap();
                other_condition.notify_one();
                panic!("汚染を注入");
            });
        });
        let mut guard = guard;
        // 条件変数の偽の起床では、汚染を確認できるまで再び待つ。
        let poison = loop {
            match lock.wait(&condition, guard) {
                Ok(next) => guard = next,
                Err(poison) => break poison,
            }
        };
        assert_eq!(poison.lock, RuntimeLockKind::DvrQueueEpoch { queue_identity: Some(7) });
        worker.join().unwrap();
    }

    use super::*;

    #[test]
    fn poison_is_permanent_counted_and_saturating() {
        let lock = PoisonTrackedMutex::new(7, RuntimeLockKind::CallbackStore);
        let _ = std::panic::catch_unwind(|| {
            let _guard = lock.lock().unwrap();
            panic!("汚染を注入");
        });
        for count in [1, 2] {
            let error = lock.lock().unwrap_err();
            assert_eq!(error.lock, RuntimeLockKind::CallbackStore);
            assert_eq!(error.poison_count, count);
            assert!(!error.counter_saturated);
        }
        lock.poison_count.store(u64::MAX - 1, Ordering::Relaxed);
        for _ in 0..2 {
            let error = lock.lock().unwrap_err();
            assert_eq!(error.poison_count, u64::MAX);
            assert!(error.counter_saturated);
        }
    }
}
