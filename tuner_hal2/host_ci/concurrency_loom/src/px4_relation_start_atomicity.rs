use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding {
    Old,
    New,
}

struct RuntimeState {
    binding: Binding,
    start_active: Arc<AtomicBool>,
}

#[test]
fn snapshot_revalidation_and_guard_acquisition_linearize_against_relation_change() {
    loom::model(|| {
        let active = Arc::new(AtomicBool::new(false));
        let runtime = Arc::new(Mutex::new(RuntimeState {
            binding: Binding::Old,
            start_active: Arc::clone(&active),
        }));
        let start_acquired = Arc::new(AtomicBool::new(false));
        let relation_changed = Arc::new(AtomicBool::new(false));

        let start_runtime = Arc::clone(&runtime);
        let start_acquired_flag = Arc::clone(&start_acquired);
        let start = loom::thread::spawn(move || {
            let state = start_runtime.lock().unwrap();
            if state.binding != Binding::Old {
                return;
            }
            if state
                .start_active
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                start_acquired_flag.store(true, Ordering::Release);
            }
        });

        let relation_runtime = Arc::clone(&runtime);
        let relation_changed_flag = Arc::clone(&relation_changed);
        let relation = loom::thread::spawn(move || {
            let mut state = relation_runtime.lock().unwrap();
            if state.start_active.load(Ordering::Acquire) {
                return;
            }
            state.binding = Binding::New;
            relation_changed_flag.store(true, Ordering::Release);
        });

        start.join().unwrap();
        relation.join().unwrap();

        let acquired = start_acquired.load(Ordering::Acquire);
        let changed = relation_changed.load(Ordering::Acquire);
        assert!(!(acquired && changed));
        let state = runtime.lock().unwrap();
        if acquired {
            assert_eq!(state.binding, Binding::Old);
        }
        if changed {
            assert_eq!(state.binding, Binding::New);
        }
    });
}

#[test]
fn relation_switch_is_blocked_by_either_affected_frontend_guard() {
    loom::model(|| {
        let old_active = Arc::new(AtomicBool::new(true));
        let new_active = Arc::new(AtomicBool::new(true));
        let changed = Arc::new(AtomicBool::new(false));

        let old_release = Arc::clone(&old_active);
        let release_old = loom::thread::spawn(move || {
            old_release.store(false, Ordering::Release);
        });

        let new_release = Arc::clone(&new_active);
        let release_new = loom::thread::spawn(move || {
            new_release.store(false, Ordering::Release);
        });

        let relation_old = Arc::clone(&old_active);
        let relation_new = Arc::clone(&new_active);
        let relation_changed = Arc::clone(&changed);
        let relation = loom::thread::spawn(move || {
            if relation_old.load(Ordering::Acquire) || relation_new.load(Ordering::Acquire) {
                return;
            }
            relation_changed.store(true, Ordering::Release);
        });

        release_old.join().unwrap();
        release_new.join().unwrap();
        relation.join().unwrap();

        if changed.load(Ordering::Acquire) {
            assert!(!old_active.load(Ordering::Acquire));
            assert!(!new_active.load(Ordering::Acquire));
        }
    });
}

#[test]
fn no_op_relation_assignment_is_not_blocked_by_an_active_guard() {
    loom::model(|| {
        let active = Arc::new(AtomicBool::new(true));
        let binding = Arc::new(Mutex::new(Binding::Old));
        let admitted = Arc::new(AtomicBool::new(false));

        let relation_binding = Arc::clone(&binding);
        let relation_admitted = Arc::clone(&admitted);
        let relation = loom::thread::spawn(move || {
            let current = *relation_binding.lock().unwrap();
            if current == Binding::Old {
                relation_admitted.store(true, Ordering::Release);
            }
        });

        relation.join().unwrap();
        assert!(active.load(Ordering::Acquire));
        assert!(admitted.load(Ordering::Acquire));
    });
}
