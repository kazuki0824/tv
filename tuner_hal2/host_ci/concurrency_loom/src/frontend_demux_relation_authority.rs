use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::Arc;

const START_ACTIVE_BIT: usize = 1;

#[derive(Debug)]
struct Authority {
    state: AtomicUsize,
}

impl Authority {
    fn new(epoch: usize) -> Self {
        Self {
            state: AtomicUsize::new(epoch),
        }
    }

    fn snapshot_epoch(&self) -> usize {
        self.state.load(Ordering::Acquire) & !START_ACTIVE_BIT
    }

    fn try_begin_start(authority: &Arc<Self>, expected_epoch: usize) -> Option<StartPermit> {
        if expected_epoch & START_ACTIVE_BIT != 0 {
            return None;
        }
        authority
            .state
            .compare_exchange(
                expected_epoch,
                expected_epoch | START_ACTIVE_BIT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()
            .map(|_| StartPermit {
                authority: Arc::clone(authority),
                epoch: expected_epoch,
            })
    }

    fn try_begin_mutation(&self) -> MutationAdmission {
        loop {
            let current = self.state.load(Ordering::Acquire);
            if current & START_ACTIVE_BIT != 0 {
                return MutationAdmission::Pending;
            }
            let Some(next) = current.checked_add(2) else {
                return MutationAdmission::Exhausted;
            };
            match self
                .state
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return MutationAdmission::Advanced(next),
                Err(actual) if actual & START_ACTIVE_BIT != 0 => {
                    return MutationAdmission::Pending;
                }
                Err(_) => loom::thread::yield_now(),
            }
        }
    }
}

struct StartPermit {
    authority: Arc<Authority>,
    epoch: usize,
}

impl Drop for StartPermit {
    fn drop(&mut self) {
        self.authority.state.store(self.epoch, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutationAdmission {
    Advanced(usize),
    Pending,
    Exhausted,
}

#[test]
fn mutation_first_rejects_stale_start_epoch() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(0));
        let stale = authority.snapshot_epoch();
        assert_eq!(
            authority.try_begin_mutation(),
            MutationAdmission::Advanced(2)
        );
        assert!(Authority::try_begin_start(&authority, stale).is_none());
    });
}

#[test]
fn start_first_keeps_mutation_epoch_stable_until_permit_drop() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(0));
        let permit = Authority::try_begin_start(&authority, 0).unwrap();
        assert_eq!(authority.try_begin_mutation(), MutationAdmission::Pending);
        assert_eq!(authority.snapshot_epoch(), 0);
        drop(permit);
        assert_eq!(authority.snapshot_epoch(), 0);
    });
}

#[test]
fn mutation_progresses_after_start_permit_drop() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(0));
        let permit = Authority::try_begin_start(&authority, 0).unwrap();
        assert_eq!(authority.try_begin_mutation(), MutationAdmission::Pending);
        drop(permit);
        assert_eq!(
            authority.try_begin_mutation(),
            MutationAdmission::Advanced(2)
        );
    });
}

#[test]
fn concurrent_mutations_do_not_lose_epoch_updates() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(0));
        let first_authority = Arc::clone(&authority);
        let first = loom::thread::spawn(move || {
            assert!(matches!(
                first_authority.try_begin_mutation(),
                MutationAdmission::Advanced(_)
            ));
        });
        let second_authority = Arc::clone(&authority);
        let second = loom::thread::spawn(move || {
            assert!(matches!(
                second_authority.try_begin_mutation(),
                MutationAdmission::Advanced(_)
            ));
        });
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(authority.snapshot_epoch(), 4);
    });
}

#[test]
fn epoch_exhaustion_does_not_wrap_or_reuse_old_epoch() {
    loom::model(|| {
        let max_epoch = usize::MAX & !START_ACTIVE_BIT;
        let authority = Authority::new(max_epoch);
        assert_eq!(authority.try_begin_mutation(), MutationAdmission::Exhausted);
        assert_eq!(authority.snapshot_epoch(), max_epoch);
    });
}
