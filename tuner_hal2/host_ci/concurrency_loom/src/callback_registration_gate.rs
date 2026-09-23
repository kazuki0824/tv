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
