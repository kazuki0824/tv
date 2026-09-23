use loom::sync::{Arc, Mutex};

struct Registration {
    current_generation: Option<u64>,
}

#[test]
fn delayed_death_from_replaced_generation_never_retires_the_current_registration() {
    loom::model(|| {
        let gate = Arc::new(Mutex::new(Registration {
            current_generation: Some(1),
        }));

        let replace_gate = Arc::clone(&gate);
        let replace = loom::thread::spawn(move || {
            replace_gate.lock().unwrap().current_generation = Some(2);
        });

        let death_gate = Arc::clone(&gate);
        let old_death = loom::thread::spawn(move || {
            let mut registration = death_gate.lock().unwrap();
            if registration.current_generation == Some(1) {
                registration.current_generation = None;
            }
        });

        replace.join().unwrap();
        old_death.join().unwrap();

        assert_eq!(gate.lock().unwrap().current_generation, Some(2));
    });
}

#[test]
fn death_of_the_current_generation_can_retire_only_that_generation() {
    loom::model(|| {
        let gate = Arc::new(Mutex::new(Registration {
            current_generation: Some(4),
        }));

        let death_gate = Arc::clone(&gate);
        let death = loom::thread::spawn(move || {
            let mut registration = death_gate.lock().unwrap();
            if registration.current_generation == Some(4) {
                registration.current_generation = None;
            }
        });

        death.join().unwrap();
        assert_eq!(gate.lock().unwrap().current_generation, None);
    });
}

#[test]
fn callback_composite_commit_and_death_reentry_preserve_lock_order() {
    use loom::sync::atomic::{AtomicBool, Ordering};

    loom::model(|| {
        let runtime = Arc::new(Mutex::new(()));
        let store = Arc::new(Mutex::new(()));
        let death_gate = Arc::new(Mutex::new(()));
        let committed = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(AtomicBool::new(false));

        let registration_runtime = Arc::clone(&runtime);
        let registration_store = Arc::clone(&store);
        let registration_gate = Arc::clone(&death_gate);
        let registration_committed = Arc::clone(&committed);
        let registration = loom::thread::spawn(move || {
            let _runtime = registration_runtime.lock().unwrap();
            loom::thread::yield_now();
            let _store = registration_store.lock().unwrap();
            loom::thread::yield_now();
            let _death = registration_gate.lock().unwrap();
            registration_committed.store(true, Ordering::Release);
        });

        let death_runtime = Arc::clone(&runtime);
        let death_store = Arc::clone(&store);
        let death_gate = Arc::clone(&death_gate);
        let death_seen = Arc::clone(&dead);
        let death = loom::thread::spawn(move || {
            {
                let _death = death_gate.lock().unwrap();
                death_seen.store(true, Ordering::Release);
            }

            let _runtime = death_runtime.lock().unwrap();
            loom::thread::yield_now();
            let _store = death_store.lock().unwrap();
        });

        registration.join().unwrap();
        death.join().unwrap();

        assert!(committed.load(Ordering::Acquire));
        assert!(dead.load(Ordering::Acquire));
    });
}
