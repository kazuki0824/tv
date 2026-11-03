use loom::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Open,
    Draining,
}

struct State {
    phase: Phase,
    epoch: u64,
    active: usize,
}

#[test]
fn drain_waits_for_old_epoch_transactions_before_publishing_the_next_epoch() {
    loom::model(|| {
        let state = Arc::new((
            Mutex::new(State {
                phase: Phase::Open,
                epoch: 11,
                active: 1,
            }),
            Condvar::new(),
        ));

        let old_token_state = Arc::clone(&state);
        let old_token = loom::thread::spawn(move || {
            let (state, wake) = &*old_token_state;
            let mut state = state.lock().unwrap();
            assert_eq!(state.epoch, 11);
            state.active -= 1;
            wake.notify_all();
        });

        let drain_state = Arc::clone(&state);
        let drain = loom::thread::spawn(move || {
            let (state, wake) = &*drain_state;
            let mut state = state.lock().unwrap();
            state.phase = Phase::Draining;
            while state.active != 0 {
                state = wake.wait(state).unwrap();
            }
            assert_eq!(state.epoch, 11);
            state.epoch = 12;
            state.phase = Phase::Open;
        });

        old_token.join().unwrap();
        drain.join().unwrap();

        let state = state.0.lock().unwrap();
        assert_eq!(state.phase, Phase::Open);
        assert_eq!(state.epoch, 12);
        assert_eq!(state.active, 0);
    });
}

#[test]
fn transaction_started_after_drain_commit_observes_only_the_new_epoch() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            phase: Phase::Draining,
            epoch: 20,
            active: 0,
        }));

        {
            let mut state = state.lock().unwrap();
            state.epoch = 21;
            state.phase = Phase::Open;
        }

        let reader_state = Arc::clone(&state);
        let reader = loom::thread::spawn(move || {
            let mut state = reader_state.lock().unwrap();
            assert_eq!(state.phase, Phase::Open);
            let epoch = state.epoch;
            state.active += 1;
            epoch
        });

        assert_eq!(reader.join().unwrap(), 21);
    });
}
