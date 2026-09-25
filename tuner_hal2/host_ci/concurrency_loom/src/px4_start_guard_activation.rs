use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    start_succeeded: bool,
    activate_completed: bool,
    cleanup_completed: bool,
    cancelled: bool,
}

fn relation_change_is_busy(guard_active: &AtomicBool) -> bool {
    guard_active.load(Ordering::Acquire)
}

#[test]
fn start_success_keeps_relation_guard_until_activate_completes() {
    loom::model(|| {
        let guard_active = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(State::default()));

        let worker_guard = Arc::clone(&guard_active);
        let worker_state = Arc::clone(&state);
        let worker = loom::thread::spawn(move || {
            {
                let mut state = worker_state.lock().unwrap();
                state.start_succeeded = true;
            }
            loom::thread::yield_now();

            {
                let mut state = worker_state.lock().unwrap();
                state.activate_completed = true;
            }
            worker_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard_active);
        let relation_state = Arc::clone(&state);
        let relation = loom::thread::spawn(move || {
            if relation_change_is_busy(&relation_guard) {
                return;
            }

            let state = relation_state.lock().unwrap();
            assert!(
                state.activate_completed,
                "relation mutation observed guard release before activate completion"
            );
        });

        worker.join().unwrap();
        relation.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.start_succeeded);
        assert!(state.activate_completed);
        assert!(!guard_active.load(Ordering::Acquire));
    });
}

#[test]
fn cancel_after_start_keeps_relation_guard_until_nonactivate_cleanup_finishes() {
    loom::model(|| {
        let guard_active = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(State {
            start_succeeded: true,
            cancelled: true,
            ..State::default()
        }));

        let cleanup_guard = Arc::clone(&guard_active);
        let cleanup_state = Arc::clone(&state);
        let cleanup = loom::thread::spawn(move || {
            loom::thread::yield_now();
            {
                let mut state = cleanup_state.lock().unwrap();
                state.cleanup_completed = true;
            }
            cleanup_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard_active);
        let relation_state = Arc::clone(&state);
        let relation = loom::thread::spawn(move || {
            if relation_change_is_busy(&relation_guard) {
                return;
            }

            let state = relation_state.lock().unwrap();
            assert!(state.cancelled);
            assert!(
                state.cleanup_completed,
                "relation mutation observed guard release before prepared pump cleanup"
            );
            assert!(!state.activate_completed);
        });

        cleanup.join().unwrap();
        relation.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.cleanup_completed);
        assert!(!state.activate_completed);
        assert!(!guard_active.load(Ordering::Acquire));
    });
}

#[test]
fn start_failure_keeps_relation_guard_until_prepared_pump_cleanup_finishes() {
    loom::model(|| {
        let guard_active = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(State::default()));

        let cleanup_guard = Arc::clone(&guard_active);
        let cleanup_state = Arc::clone(&state);
        let cleanup = loom::thread::spawn(move || {
            loom::thread::yield_now();
            {
                let mut state = cleanup_state.lock().unwrap();
                state.cleanup_completed = true;
            }
            cleanup_guard.store(false, Ordering::Release);
        });

        let relation_guard = Arc::clone(&guard_active);
        let relation_state = Arc::clone(&state);
        let relation = loom::thread::spawn(move || {
            if relation_change_is_busy(&relation_guard) {
                return;
            }

            let state = relation_state.lock().unwrap();
            assert!(!state.start_succeeded);
            assert!(
                state.cleanup_completed,
                "relation mutation observed guard release before failure cleanup"
            );
        });

        cleanup.join().unwrap();
        relation.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.cleanup_completed);
        assert!(!guard_active.load(Ordering::Acquire));
    });
}
