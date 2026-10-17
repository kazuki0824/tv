use loom::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Open,
    Draining,
}

struct State {
    phase: Phase,
    permits: usize,
    generation: u64,
}

#[test]
fn drain_rejects_new_producers_and_advances_generation_after_all_permits_leave() {
    loom::model(|| {
        let state = Arc::new((
            Mutex::new(State {
                phase: Phase::Open,
                permits: 1,
                generation: 7,
            }),
            Condvar::new(),
        ));

        let drain_state = Arc::clone(&state);
        let drain = loom::thread::spawn(move || {
            let (state, wake) = &*drain_state;
            let mut state = state.lock().unwrap();
            state.phase = Phase::Draining;
            while state.permits != 0 {
                state = wake.wait(state).unwrap();
            }
            state.generation += 1;
            state.phase = Phase::Open;
        });

        let producer_state = Arc::clone(&state);
        let producer = loom::thread::spawn(move || {
            let (state, wake) = &*producer_state;
            let mut state = state.lock().unwrap();
            assert_eq!(state.generation, 7);
            state.permits -= 1;
            wake.notify_all();
        });

        producer.join().unwrap();
        drain.join().unwrap();

        let state = state.0.lock().unwrap();
        assert_eq!(state.phase, Phase::Open);
        assert_eq!(state.permits, 0);
        assert_eq!(state.generation, 8);
    });
}

#[test]
fn producer_admission_never_crosses_an_observed_draining_phase() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            phase: Phase::Open,
            permits: 0,
            generation: 3,
        }));

        let drain_state = Arc::clone(&state);
        let drain = loom::thread::spawn(move || {
            let mut state = drain_state.lock().unwrap();
            state.phase = Phase::Draining;
        });

        let producer_state = Arc::clone(&state);
        let producer = loom::thread::spawn(move || {
            let mut state = producer_state.lock().unwrap();
            if state.phase == Phase::Open {
                state.permits += 1;
                true
            } else {
                false
            }
        });

        let admitted = producer.join().unwrap();
        drain.join().unwrap();

        let state = state.lock().unwrap();
        if state.phase == Phase::Draining {
            assert_eq!(state.permits, usize::from(admitted));
        }
    });
}
