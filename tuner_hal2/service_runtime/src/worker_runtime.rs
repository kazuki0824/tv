use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub use maleicacid_tuner_hal2_control_core::{
    WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor,
    WorkerTerminalResult,
};

pub const CLEANUP_RETRY_SCHEDULE_MS: &[u64] = &[0, 10, 100, 1_000];
pub const CLEANUP_TERMINAL_DEADLINE_MS: u64 = 30_000;
pub const WORKER_IO_DEADLINE_MS: u64 = 2_000;
pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;

struct FilterDeliveryWake {
    sequence: Mutex<u64>,
    changed: Condvar,
}

fn filter_delivery_wake() -> &'static FilterDeliveryWake {
    static WAKE: OnceLock<FilterDeliveryWake> = OnceLock::new();
    WAKE.get_or_init(|| FilterDeliveryWake {
        sequence: Mutex::new(0),
        changed: Condvar::new(),
    })
}

fn filter_delivery_sequence_lock() -> MutexGuard<'static, u64> {
    match filter_delivery_wake().sequence.lock() {
        Ok(sequence) => sequence,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn filter_delivery_wake_sequence() -> u64 {
    *filter_delivery_sequence_lock()
}

pub(crate) fn notify_filter_delivery_change() {
    let wake = filter_delivery_wake();
    let mut sequence = filter_delivery_sequence_lock();
    *sequence = sequence.wrapping_add(1);
    wake.changed.notify_all();
}

pub(crate) fn wait_filter_delivery_change(observed: u64, deadline: Option<Instant>) -> u64 {
    let wake = filter_delivery_wake();
    let mut sequence = filter_delivery_sequence_lock();
    loop {
        if *sequence != observed {
            return *sequence;
        }
        match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return *sequence;
                }
                sequence = match wake
                    .changed
                    .wait_timeout(sequence, deadline.saturating_duration_since(now))
                {
                    Ok((sequence, _)) => sequence,
                    Err(poisoned) => poisoned.into_inner().0,
                };
            }
            None => {
                sequence = match wake.changed.wait(sequence) {
                    Ok(sequence) => sequence,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }
        }
    }
}

pub fn join_worker_classified<T>(
    worker: WorkerRuntime<T>,
) -> crate::worker_failure_classifier::ClassifiedWorkerTerminalResult<T> {
    crate::worker_failure_classifier::WorkerFailureClassifier::classify_terminal(
        worker.join(),
        "worker panicked or could not be joined",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn filter_delivery_wake_is_not_lost_between_snapshot_and_wait() {
        let observed = filter_delivery_wake_sequence();
        notify_filter_delivery_change();
        let next = wait_filter_delivery_change(
            observed,
            Some(Instant::now() + Duration::from_millis(100)),
        );
        assert_ne!(next, observed);
    }

    #[test]
    fn filter_delivery_wait_returns_at_deadline_without_a_notification() {
        let observed = filter_delivery_wake_sequence();
        let started = Instant::now();
        let next = wait_filter_delivery_change(
            observed,
            Some(started + Duration::from_millis(5)),
        );
        assert_eq!(next, observed);
        assert!(started.elapsed() >= Duration::from_millis(5));
    }

    #[test]
    fn filter_delivery_notification_wakes_all_waiters_without_consuming_the_signal() {
        let observed = filter_delivery_wake_sequence();
        let results = Arc::new(Mutex::new(Vec::new()));
        let mut joins = Vec::new();
        for _ in 0..2 {
            let results = Arc::clone(&results);
            joins.push(thread::spawn(move || {
                let next = wait_filter_delivery_change(
                    observed,
                    Some(Instant::now() + Duration::from_millis(200)),
                );
                results.lock().unwrap().push(next);
            }));
        }
        thread::sleep(Duration::from_millis(5));
        notify_filter_delivery_change();
        for join in joins {
            join.join().unwrap();
        }
        let results = results.lock().unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|next| *next != observed));
    }
}
