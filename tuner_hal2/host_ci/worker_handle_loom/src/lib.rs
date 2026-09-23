#![cfg(loom)]

#[cfg(test)]
mod tests {
    use loom::sync::atomic::{AtomicBool, Ordering};
    use loom::sync::{Arc, Condvar, Mutex};

    #[test]
    fn completion_publication_precedes_thread_exit_and_collection_must_join() {
        loom::model(|| {
            let result = Arc::new(Mutex::new(None::<u32>));
            let completion = Arc::new((Mutex::new(false), Condvar::new()));
            let allow_exit = Arc::new((Mutex::new(false), Condvar::new()));
            let exited = Arc::new(AtomicBool::new(false));

            let worker_result = Arc::clone(&result);
            let worker_completion = Arc::clone(&completion);
            let worker_allow_exit = Arc::clone(&allow_exit);
            let worker_exited = Arc::clone(&exited);
            let worker = loom::thread::spawn(move || {
                *worker_result.lock().unwrap() = Some(7);
                {
                    let (completed, wake) = &*worker_completion;
                    *completed.lock().unwrap() = true;
                    wake.notify_all();
                }

                // 完了公開後も物理スレッド終了前の窓が存在することを固定する。
                let (allowed, wake) = &*worker_allow_exit;
                let mut allowed = allowed.lock().unwrap();
                while !*allowed {
                    allowed = wake.wait(allowed).unwrap();
                }
                worker_exited.store(true, Ordering::SeqCst);
            });

            {
                let (completed, wake) = &*completion;
                let mut completed = completed.lock().unwrap();
                while !*completed {
                    completed = wake.wait(completed).unwrap();
                }
            }

            // 旧実装のJoinHandle::is_finished相当を先に判定するとRunningへ戻り得る窓。
            assert!(!exited.load(Ordering::SeqCst));

            {
                let (allowed, wake) = &*allow_exit;
                *allowed.lock().unwrap() = true;
                wake.notify_all();
            }

            // 完了公開後の回収は物理終了をjoinしてから結果を確定する。
            worker.join().unwrap();
            assert!(exited.load(Ordering::SeqCst));
            assert_eq!(result.lock().unwrap().take(), Some(7));
        });
    }
}
