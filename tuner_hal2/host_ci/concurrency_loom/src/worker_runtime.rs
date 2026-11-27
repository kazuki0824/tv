use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};

#[test]
fn same_reaper_key_is_coalesced_until_the_owned_work_finishes() {
    loom::model(|| {
        #[derive(Default)]
        struct State {
            pending: bool,
            running: bool,
        }

        let state = Arc::new(Mutex::new(State::default()));
        let scheduled = Arc::new(AtomicUsize::new(0));

        let enqueue = |state: Arc<Mutex<State>>, scheduled: Arc<AtomicUsize>| {
            loom::thread::spawn(move || {
                let mut state = state.lock().unwrap();
                if state.pending || state.running {
                    return;
                }
                state.pending = true;
                scheduled.fetch_add(1, Ordering::SeqCst);
            })
        };

        let first = enqueue(Arc::clone(&state), Arc::clone(&scheduled));
        let second = enqueue(Arc::clone(&state), Arc::clone(&scheduled));
        first.join().unwrap();
        second.join().unwrap();

        assert_eq!(scheduled.load(Ordering::SeqCst), 1);

        {
            let mut state = state.lock().unwrap();
            assert!(state.pending);
            state.pending = false;
            state.running = true;
        }

        let duplicate_state = Arc::clone(&state);
        let duplicate_scheduled = Arc::clone(&scheduled);
        let duplicate = loom::thread::spawn(move || {
            let mut state = duplicate_state.lock().unwrap();
            if !state.pending && !state.running {
                state.pending = true;
                duplicate_scheduled.fetch_add(1, Ordering::SeqCst);
            }
        });
        duplicate.join().unwrap();
        assert_eq!(scheduled.load(Ordering::SeqCst), 1);

        state.lock().unwrap().running = false;

        let next_state = Arc::clone(&state);
        let next_scheduled = Arc::clone(&scheduled);
        let next = loom::thread::spawn(move || {
            let mut state = next_state.lock().unwrap();
            if !state.pending && !state.running {
                state.pending = true;
                next_scheduled.fetch_add(1, Ordering::SeqCst);
            }
        });
        next.join().unwrap();
        assert_eq!(scheduled.load(Ordering::SeqCst), 2);
    });
}
