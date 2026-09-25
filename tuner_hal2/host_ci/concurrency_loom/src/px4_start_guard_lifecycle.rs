use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};

#[derive(Default)]
struct PhaseState {
    start_succeeded: bool,
    cancelled: bool,
    activate_completed: bool,
    cleanup_completed: bool,
}

#[test]
fn success_releases_guard_only_after_activate_completion() {
    loom::model(|| {
        let guard = Arc::new(AtomicBool::new(true));
        let phase = Arc::new(Mutex::new(PhaseState::default()));

        let worker_guard = Arc::clone(&guard);
        let worker_phase = Arc::clone(&phase);
        let worker = loom::thread::spawn(move || {
            worker_phase.lock().unwrap().start_succeeded = true;
            loom::thread::yield_now();
            worker_phase.lock().unwrap().activate_completed = true;
            worker_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard);
        let relation_phase = Arc::clone(&phase);
        let relation = loom::thread::spawn(move || {
            if !relation_guard.load(Ordering::Acquire) {
                assert!(relation_phase.lock().unwrap().activate_completed);
            }
        });

        worker.join().unwrap();
        relation.join().unwrap();

        assert!(phase.lock().unwrap().activate_completed);
        assert!(!guard.load(Ordering::Acquire));
    });
}

#[test]
fn start_failure_releases_guard_only_after_prepared_cleanup() {
    loom::model(|| {
        let guard = Arc::new(AtomicBool::new(true));
        let phase = Arc::new(Mutex::new(PhaseState::default()));

        let worker_guard = Arc::clone(&guard);
        let worker_phase = Arc::clone(&phase);
        let worker = loom::thread::spawn(move || {
            loom::thread::yield_now();
            worker_phase.lock().unwrap().cleanup_completed = true;
            worker_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard);
        let relation_phase = Arc::clone(&phase);
        let relation = loom::thread::spawn(move || {
            if !relation_guard.load(Ordering::Acquire) {
                let phase = relation_phase.lock().unwrap();
                assert!(!phase.start_succeeded);
                assert!(phase.cleanup_completed);
            }
        });

        worker.join().unwrap();
        relation.join().unwrap();

        assert!(phase.lock().unwrap().cleanup_completed);
        assert!(!guard.load(Ordering::Acquire));
    });
}

#[test]
fn cancel_after_start_releases_guard_only_after_nonactivate_cleanup() {
    loom::model(|| {
        let guard = Arc::new(AtomicBool::new(true));
        let phase = Arc::new(Mutex::new(PhaseState {
            start_succeeded: true,
            cancelled: true,
            ..PhaseState::default()
        }));

        let worker_guard = Arc::clone(&guard);
        let worker_phase = Arc::clone(&phase);
        let worker = loom::thread::spawn(move || {
            loom::thread::yield_now();
            worker_phase.lock().unwrap().cleanup_completed = true;
            worker_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard);
        let relation_phase = Arc::clone(&phase);
        let relation = loom::thread::spawn(move || {
            if !relation_guard.load(Ordering::Acquire) {
                let phase = relation_phase.lock().unwrap();
                assert!(phase.start_succeeded);
                assert!(phase.cancelled);
                assert!(phase.cleanup_completed);
                assert!(!phase.activate_completed);
            }
        });

        worker.join().unwrap();
        relation.join().unwrap();

        let phase = phase.lock().unwrap();
        assert!(phase.cleanup_completed);
        assert!(!phase.activate_completed);
        assert!(!guard.load(Ordering::Acquire));
    });
}

#[test]
fn concurrent_start_guard_acquisition_has_one_winner() {
    loom::model(|| {
        let active = Arc::new(AtomicBool::new(false));
        let winners = Arc::new(AtomicUsize::new(0));

        let acquire = |active: Arc<AtomicBool>, winners: Arc<AtomicUsize>| {
            loom::thread::spawn(move || {
                if active
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
            })
        };

        let first = acquire(Arc::clone(&active), Arc::clone(&winners));
        let second = acquire(Arc::clone(&active), Arc::clone(&winners));
        first.join().unwrap();
        second.join().unwrap();

        assert_eq!(winners.load(Ordering::SeqCst), 1);
        assert!(active.load(Ordering::Acquire));
    });
}
