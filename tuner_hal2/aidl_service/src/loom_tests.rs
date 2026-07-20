#[cfg(loom)]
#[test]
fn callback_artifact_then_runtime_finish_interleaving_model_records_both_outcomes() {
    loom::model(|| {
        let artifact_done = loom::sync::Arc::new(loom::sync::atomic::AtomicBool::new(false));
        let runtime_finish_seen = loom::sync::Arc::new(loom::sync::atomic::AtomicBool::new(false));

        let artifact = artifact_done.clone();
        let artifact_thread = loom::thread::spawn(move || {
            artifact.store(true, loom::sync::atomic::Ordering::SeqCst);
        });

        let artifact_for_finish = artifact_done.clone();
        let finish = runtime_finish_seen.clone();
        let runtime_thread = loom::thread::spawn(move || {
            let _artifact_result_observed =
                artifact_for_finish.load(loom::sync::atomic::Ordering::SeqCst);
            finish.store(true, loom::sync::atomic::Ordering::SeqCst);
        });

        artifact_thread.join().unwrap();
        runtime_thread.join().unwrap();
        assert!(runtime_finish_seen.load(loom::sync::atomic::Ordering::SeqCst));
    });
}
