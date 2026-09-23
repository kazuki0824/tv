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

            assert!(!exited.load(Ordering::SeqCst));
            {
                let (allowed, wake) = &*allow_exit;
                *allowed.lock().unwrap() = true;
                wake.notify_all();
            }

            worker.join().unwrap();
            assert!(exited.load(Ordering::SeqCst));
            assert_eq!(result.lock().unwrap().take(), Some(7));
        });
    }

    #[test]
    fn supervisor_start_stop_restart_keeps_the_restart_request() {
        #[derive(Default)]
        struct State {
            starting: bool,
            cancel: bool,
            restart: bool,
            reaping: bool,
        }

        loom::model(|| {
            let state = Arc::new(Mutex::new(State {
                starting: true,
                ..State::default()
            }));
            let operation_entered = Arc::new((Mutex::new(false), Condvar::new()));
            let operation_release = Arc::new((Mutex::new(false), Condvar::new()));

            let worker_state = Arc::clone(&state);
            let entered = Arc::clone(&operation_entered);
            let release = Arc::clone(&operation_release);
            let worker = loom::thread::spawn(move || {
                {
                    let (entered, wake) = &*entered;
                    *entered.lock().unwrap() = true;
                    wake.notify_all();
                }
                {
                    let (released, wake) = &*release;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = wake.wait(released).unwrap();
                    }
                }
                let mut state = worker_state.lock().unwrap();
                assert!(state.starting);
                state.starting = false;
                state.reaping = state.cancel;
            });

            {
                let (entered, wake) = &*operation_entered;
                let mut entered = entered.lock().unwrap();
                while !*entered {
                    entered = wake.wait(entered).unwrap();
                }
            }

            {
                let mut state = state.lock().unwrap();
                assert!(state.starting);
                state.cancel = true;
            }
            {
                let mut state = state.lock().unwrap();
                assert!(state.starting && state.cancel);
                state.restart = true;
            }
            {
                let (released, wake) = &*operation_release;
                *released.lock().unwrap() = true;
                wake.notify_all();
            }

            worker.join().unwrap();
            let state = state.lock().unwrap();
            assert!(!state.starting);
            assert!(state.reaping);
            assert!(state.restart);
        });
    }

    #[test]
    fn executing_cleanup_excludes_reissue_until_completion() {
        #[derive(Clone, Copy, Eq, PartialEq)]
        enum Phase {
            Ready,
            Executing,
            Completed,
        }

        loom::model(|| {
            let phase = Arc::new(Mutex::new(Phase::Ready));
            let entered = Arc::new((Mutex::new(false), Condvar::new()));
            let release = Arc::new((Mutex::new(false), Condvar::new()));

            let worker_phase = Arc::clone(&phase);
            let worker_entered = Arc::clone(&entered);
            let worker_release = Arc::clone(&release);
            let worker = loom::thread::spawn(move || {
                {
                    let mut phase = worker_phase.lock().unwrap();
                    assert!(matches!(*phase, Phase::Ready));
                    *phase = Phase::Executing;
                }
                {
                    let (entered, wake) = &*worker_entered;
                    *entered.lock().unwrap() = true;
                    wake.notify_all();
                }
                {
                    let (released, wake) = &*worker_release;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = wake.wait(released).unwrap();
                    }
                }
                *worker_phase.lock().unwrap() = Phase::Completed;
            });

            {
                let (entered, wake) = &*entered;
                let mut entered = entered.lock().unwrap();
                while !*entered {
                    entered = wake.wait(entered).unwrap();
                }
            }
            assert!(matches!(*phase.lock().unwrap(), Phase::Executing));

            {
                let (released, wake) = &*release;
                *released.lock().unwrap() = true;
                wake.notify_all();
            }
            worker.join().unwrap();
            assert!(matches!(*phase.lock().unwrap(), Phase::Completed));
        });
    }

    #[test]
    fn callback_death_and_registration_commit_share_one_gate() {
        loom::model(|| {
            let gate = Arc::new(Mutex::new(()));
            let dead = Arc::new(AtomicBool::new(false));
            let commit_completed = Arc::new(AtomicBool::new(false));

            let death_gate = Arc::clone(&gate);
            let death_dead = Arc::clone(&dead);
            let death = loom::thread::spawn(move || {
                let _guard = death_gate.lock().unwrap();
                death_dead.store(true, Ordering::SeqCst);
            });

            let commit_gate = Arc::clone(&gate);
            let commit_done = Arc::clone(&commit_completed);
            let commit = loom::thread::spawn(move || {
                let _guard = commit_gate.lock().unwrap();
                commit_done.store(true, Ordering::SeqCst);
            });

            death.join().unwrap();
            commit.join().unwrap();
            assert!(dead.load(Ordering::SeqCst));
            assert!(commit_completed.load(Ordering::SeqCst));
        });
    }

    #[test]
    fn queue_drain_publishes_new_epoch_only_after_active_transaction_finishes() {
        struct QueueState {
            draining: bool,
            active: usize,
            epoch: usize,
        }

        loom::model(|| {
            let state = Arc::new((
                Mutex::new(QueueState {
                    draining: false,
                    active: 1,
                    epoch: 4,
                }),
                Condvar::new(),
            ));

            let drain_state = Arc::clone(&state);
            let drain = loom::thread::spawn(move || {
                let (state, wake) = &*drain_state;
                let mut state = state.lock().unwrap();
                state.draining = true;
                while state.active != 0 {
                    state = wake.wait(state).unwrap();
                }
                state.epoch += 1;
                state.draining = false;
            });

            let producer_state = Arc::clone(&state);
            let producer = loom::thread::spawn(move || {
                let (state, wake) = &*producer_state;
                let mut state = state.lock().unwrap();
                assert_eq!(state.epoch, 4);
                state.active -= 1;
                wake.notify_all();
            });

            producer.join().unwrap();
            drain.join().unwrap();
            let state = state.0.lock().unwrap();
            assert_eq!(state.active, 0);
            assert_eq!(state.epoch, 5);
            assert!(!state.draining);
        });
    }

    #[test]
    fn wake_before_wait_is_retained_by_the_pending_flag() {
        loom::model(|| {
            let pending = Arc::new(AtomicBool::new(false));
            let published = Arc::new(AtomicBool::new(false));

            let notifier_pending = Arc::clone(&pending);
            let notifier_published = Arc::clone(&published);
            let notifier = loom::thread::spawn(move || {
                notifier_pending.store(true, Ordering::Release);
                notifier_published.store(true, Ordering::Release);
            });

            let waiter_pending = Arc::clone(&pending);
            let waiter_published = Arc::clone(&published);
            let waiter = loom::thread::spawn(move || {
                while !waiter_published.load(Ordering::Acquire) {
                    loom::thread::yield_now();
                }
                assert!(waiter_pending.swap(false, Ordering::AcqRel));
            });

            notifier.join().unwrap();
            waiter.join().unwrap();
            assert!(!pending.load(Ordering::Acquire));
        });
    }
}

#[cfg(test)]
mod callback_registration_gate;
#[cfg(test)]
mod filter_producer_drain_gate;
#[cfg(test)]
mod frontend_backend_submit_ticket;
#[cfg(test)]
mod frontend_worker_cancel_order;
#[cfg(test)]
mod lnb_registry_io_authority;
#[cfg(test)]
mod queue_epoch_protocol;
#[cfg(test)]
mod worker_runtime;
