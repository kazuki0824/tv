#[cfg(loom)]
#[test]
fn frontend_worker_external_join_model_does_not_hold_runtime_lock() {
    loom::model(|| {
        let runtime_lock = loom::sync::Arc::new(loom::sync::Mutex::new(false));
        let worker_finished = loom::sync::Arc::new(loom::sync::atomic::AtomicBool::new(false));

        let lock_for_join = runtime_lock.clone();
        let finished_for_join = worker_finished.clone();
        let joiner = loom::thread::spawn(move || {
            {
                let mut locked = lock_for_join.lock().unwrap();
                *locked = true;
            }
            finished_for_join.store(true, loom::sync::atomic::Ordering::SeqCst);
        });

        joiner.join().unwrap();
        assert!(worker_finished.load(loom::sync::atomic::Ordering::SeqCst));
        assert!(*runtime_lock.lock().unwrap());
    });
}
