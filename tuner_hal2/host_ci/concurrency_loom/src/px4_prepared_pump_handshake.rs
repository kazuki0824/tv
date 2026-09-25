use loom::sync::{Arc, Condvar, Mutex};

#[derive(Default)]
struct PumpState {
    ready: bool,
    activate: bool,
    stop: bool,
    read_started: bool,
}

fn spawn_prepared_pump(state: Arc<(Mutex<PumpState>, Condvar)>) -> loom::thread::JoinHandle<()> {
    loom::thread::spawn(move || {
        let (state, wake) = &*state;
        let mut state = state.lock().unwrap();
        state.ready = true;
        wake.notify_all();

        while !state.activate && !state.stop {
            state = wake.wait(state).unwrap();
        }

        if state.stop {
            return;
        }

        assert!(state.activate);
        state.read_started = true;
    })
}

#[test]
fn cancel_while_ready_is_pending_never_starts_reading() {
    loom::model(|| {
        let state = Arc::new((Mutex::new(PumpState::default()), Condvar::new()));
        let child = spawn_prepared_pump(Arc::clone(&state));

        let caller_state = Arc::clone(&state);
        let caller = loom::thread::spawn(move || {
            let (state, wake) = &*caller_state;
            let mut state = state.lock().unwrap();
            state.stop = true;
            wake.notify_all();
        });

        caller.join().unwrap();
        child.join().unwrap();

        let state = state.0.lock().unwrap();
        assert!(state.stop);
        assert!(!state.read_started);
    });
}

#[test]
fn activate_releases_the_prepared_pump_only_after_ready_publication() {
    loom::model(|| {
        let state = Arc::new((Mutex::new(PumpState::default()), Condvar::new()));
        let child = spawn_prepared_pump(Arc::clone(&state));

        let caller_state = Arc::clone(&state);
        let caller = loom::thread::spawn(move || {
            let (state, wake) = &*caller_state;
            let mut state = state.lock().unwrap();
            while !state.ready {
                state = wake.wait(state).unwrap();
            }
            state.activate = true;
            wake.notify_all();
        });

        caller.join().unwrap();
        child.join().unwrap();

        let state = state.0.lock().unwrap();
        assert!(state.ready);
        assert!(state.activate);
        assert!(state.read_started);
        assert!(!state.stop);
    });
}

#[test]
fn stop_published_before_activation_prevents_read_start() {
    loom::model(|| {
        let state = Arc::new((Mutex::new(PumpState::default()), Condvar::new()));

        {
            let mut state = state.0.lock().unwrap();
            state.stop = true;
            state.activate = true;
        }

        let child = spawn_prepared_pump(Arc::clone(&state));
        child.join().unwrap();

        let state = state.0.lock().unwrap();
        assert!(state.stop);
        assert!(state.activate);
        assert!(!state.read_started);
    });
}
