use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};

#[test]
fn observing_stop_also_observes_the_published_cancel_reason() {
    loom::model(|| {
        let cancel_reason = Arc::new(Mutex::new(None::<u8>));
        let stop = Arc::new(AtomicBool::new(false));

        let requester_reason = Arc::clone(&cancel_reason);
        let requester_stop = Arc::clone(&stop);
        let requester = loom::thread::spawn(move || {
            *requester_reason.lock().unwrap() = Some(7);
            requester_stop.store(true, Ordering::Release);
        });

        let worker_reason = Arc::clone(&cancel_reason);
        let worker_stop = Arc::clone(&stop);
        let worker = loom::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                loom::thread::yield_now();
            }
            assert_eq!(*worker_reason.lock().unwrap(), Some(7));
        });

        requester.join().unwrap();
        worker.join().unwrap();
    });
}
