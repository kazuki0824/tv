use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};

struct State {
    generation: u64,
}

#[test]
fn io_authority_revalidates_generation_after_waiting_for_the_physical_gate() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State { generation: 1 }));
        let io_gate = Arc::new(Mutex::new(()));
        let applied = Arc::new(AtomicUsize::new(0));

        let request_state = Arc::clone(&state);
        let request_gate = Arc::clone(&io_gate);
        let request_applied = Arc::clone(&applied);
        let (observed_tx, observed_rx) = loom::sync::mpsc::channel();
        let held_gate = io_gate.lock().unwrap();
        let request = loom::thread::spawn(move || {
            let observed_generation = request_state.lock().unwrap().generation;
            observed_tx.send(()).unwrap();
            let _io = request_gate.lock().unwrap();
            let current_generation = request_state.lock().unwrap().generation;
            if current_generation == observed_generation {
                request_applied.fetch_add(1, Ordering::SeqCst);
            }
        });

        observed_rx.recv().unwrap();
        state.lock().unwrap().generation += 1;
        drop(held_gate);
        request.join().unwrap();
        assert_eq!(applied.load(Ordering::SeqCst), 0);
        assert_eq!(state.lock().unwrap().generation, 2);
    });
}

#[test]
fn one_physical_lnb_gate_serializes_all_backend_effects() {
    loom::model(|| {
        let gate = Arc::new(Mutex::new(()));
        let in_backend = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let run =
            |gate: Arc<Mutex<()>>, in_backend: Arc<AtomicUsize>, max_seen: Arc<AtomicUsize>| {
                loom::thread::spawn(move || {
                    let _guard = gate.lock().unwrap();
                    let now = in_backend.fetch_add(1, Ordering::SeqCst) + 1;
                    let old = max_seen.load(Ordering::SeqCst);
                    if now > old {
                        max_seen.store(now, Ordering::SeqCst);
                    }
                    in_backend.fetch_sub(1, Ordering::SeqCst);
                })
            };

        let first = run(
            Arc::clone(&gate),
            Arc::clone(&in_backend),
            Arc::clone(&max_seen),
        );
        let second = run(gate, in_backend, Arc::clone(&max_seen));
        first.join().unwrap();
        second.join().unwrap();

        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    });
}
