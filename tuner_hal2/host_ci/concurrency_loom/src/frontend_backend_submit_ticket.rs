use loom::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Disposition {
    Claim,
    Abort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    Claimed,
    Aborted,
}

#[derive(Debug)]
struct State {
    ready_submitted: bool,
    owner_alive: bool,
    disposition: Option<Disposition>,
    outcome: Option<Outcome>,
}

fn submit_worker(state: Arc<Mutex<State>>) -> loom::thread::JoinHandle<()> {
    loom::thread::spawn(move || {
        {
            let mut state = state.lock().unwrap();
            if !state.owner_alive {
                state.outcome = Some(Outcome::Aborted);
                return;
            }
            state.ready_submitted = true;
        }

        loop {
            let mut state = state.lock().unwrap();
            match state.disposition {
                Some(Disposition::Claim) => {
                    state.outcome = Some(Outcome::Claimed);
                    return;
                }
                Some(Disposition::Abort) => {
                    state.outcome = Some(Outcome::Aborted);
                    return;
                }
                None if !state.owner_alive => {
                    state.outcome = Some(Outcome::Aborted);
                    return;
                }
                None => {}
            }
            drop(state);
            loom::thread::yield_now();
        }
    })
}

#[test]
fn timeout_or_drop_never_turns_a_submitted_session_into_claimed() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            ready_submitted: false,
            owner_alive: true,
            disposition: None,
            outcome: None,
        }));

        let submit = submit_worker(Arc::clone(&state));

        let drop_state = Arc::clone(&state);
        let timeout_or_drop = loom::thread::spawn(move || {
            let mut state = drop_state.lock().unwrap();
            if state.disposition.is_none() {
                state.disposition = Some(Disposition::Abort);
            }
            state.owner_alive = false;
        });

        timeout_or_drop.join().unwrap();
        submit.join().unwrap();

        assert_eq!(state.lock().unwrap().outcome, Some(Outcome::Aborted));
    });
}

#[test]
fn claim_after_ready_publication_completes_as_claimed() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            ready_submitted: false,
            owner_alive: true,
            disposition: None,
            outcome: None,
        }));

        let submit = submit_worker(Arc::clone(&state));

        let claim_state = Arc::clone(&state);
        let claimant = loom::thread::spawn(move || loop {
            let mut state = claim_state.lock().unwrap();
            if state.ready_submitted {
                state.disposition = Some(Disposition::Claim);
                return;
            }
            drop(state);
            loom::thread::yield_now();
        });

        claimant.join().unwrap();
        submit.join().unwrap();

        assert_eq!(state.lock().unwrap().outcome, Some(Outcome::Claimed));
    });
}

#[test]
fn receiver_loss_before_ready_publication_aborts_without_claiming() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            ready_submitted: false,
            owner_alive: true,
            disposition: None,
            outcome: None,
        }));

        let owner_state = Arc::clone(&state);
        let owner_drop = loom::thread::spawn(move || {
            let mut state = owner_state.lock().unwrap();
            state.owner_alive = false;
        });
        owner_drop.join().unwrap();

        let submit = submit_worker(Arc::clone(&state));
        submit.join().unwrap();

        let state = state.lock().unwrap();
        assert!(!state.ready_submitted);
        assert_eq!(state.outcome, Some(Outcome::Aborted));
    });
}
