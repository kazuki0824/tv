use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::Arc;

const START_ACTIVE_BIT: usize = 1;
const MUTATION_ACTIVE_BIT: usize = 2;
const ACTIVE_MASK: usize = START_ACTIVE_BIT | MUTATION_ACTIVE_BIT;
const EPOCH_STEP: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RelationEpoch(usize);

impl RelationEpoch {
    const fn raw(self) -> usize {
        self.0
    }
}

#[derive(Debug)]
struct Authority {
    state: AtomicUsize,
}

impl Authority {
    fn new(epoch: RelationEpoch) -> Self {
        Self {
            state: AtomicUsize::new(epoch.raw()),
        }
    }

    fn snapshot_epoch(&self) -> RelationEpoch {
        RelationEpoch(self.state.load(Ordering::Acquire) & !ACTIVE_MASK)
    }

    fn try_begin_start(
        authority: &Arc<Self>,
        expected_epoch: RelationEpoch,
    ) -> Option<StartPermit> {
        let expected_raw = expected_epoch.raw();
        authority
            .state
            .compare_exchange(
                expected_raw,
                expected_raw | START_ACTIVE_BIT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()
            .map(|_| StartPermit {
                authority: Arc::clone(authority),
                epoch: expected_epoch,
            })
    }

    fn try_reserve_mutation(
        authority: &Arc<Self>,
    ) -> Result<Option<MutationLease>, MutationAdmission> {
        loop {
            let current = authority.state.load(Ordering::Acquire);
            if current & ACTIVE_MASK != 0 {
                return Ok(None);
            }
            let Some(next) = current.checked_add(EPOCH_STEP) else {
                return Err(MutationAdmission::Exhausted);
            };
            match authority.state.compare_exchange(
                current,
                current | MUTATION_ACTIVE_BIT,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(Some(MutationLease {
                        authority: Arc::clone(authority),
                        epoch: RelationEpoch(current),
                        next_epoch: RelationEpoch(next),
                        active: true,
                    }))
                }
                Err(actual) if actual & ACTIVE_MASK != 0 => return Ok(None),
                Err(_) => loom::thread::yield_now(),
            }
        }
    }
}

struct StartPermit {
    authority: Arc<Authority>,
    epoch: RelationEpoch,
}

impl Drop for StartPermit {
    fn drop(&mut self) {
        self.authority
            .state
            .store(self.epoch.raw(), Ordering::Release);
    }
}

#[derive(Debug)]
struct MutationLease {
    authority: Arc<Authority>,
    epoch: RelationEpoch,
    next_epoch: RelationEpoch,
    active: bool,
}

impl MutationLease {
    fn commit(mut self) {
        self.authority
            .state
            .store(self.next_epoch.raw(), Ordering::Release);
        self.active = false;
    }
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        if self.active {
            self.authority
                .state
                .store(self.epoch.raw(), Ordering::Release);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutationAdmission {
    Pending,
    Exhausted,
}

fn try_reserve_all(
    authorities: &[Arc<Authority>],
) -> Result<Vec<MutationLease>, MutationAdmission> {
    let mut leases = Vec::with_capacity(authorities.len());
    for authority in authorities {
        match Authority::try_reserve_mutation(authority)? {
            Some(lease) => leases.push(lease),
            None => {
                drop(leases);
                return Err(MutationAdmission::Pending);
            }
        }
    }
    Ok(leases)
}

#[test]
fn mutation_first_rejects_stale_start_epoch() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(RelationEpoch(0)));
        let stale = authority.snapshot_epoch();
        let leases = try_reserve_all(&[Arc::clone(&authority)]).unwrap();
        for lease in leases {
            lease.commit();
        }
        assert!(Authority::try_begin_start(&authority, stale).is_none());
    });
}

#[test]
fn start_first_keeps_mutation_epoch_stable_until_permit_drop() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(RelationEpoch(0)));
        let permit = Authority::try_begin_start(&authority, RelationEpoch(0)).unwrap();
        assert_eq!(
            try_reserve_all(&[Arc::clone(&authority)]).unwrap_err(),
            MutationAdmission::Pending
        );
        assert_eq!(authority.snapshot_epoch(), RelationEpoch(0));
        drop(permit);
        assert_eq!(authority.snapshot_epoch(), RelationEpoch(0));
    });
}

#[test]
fn mutation_progresses_after_start_permit_drop() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(RelationEpoch(0)));
        let permit = Authority::try_begin_start(&authority, RelationEpoch(0)).unwrap();
        assert_eq!(
            try_reserve_all(&[Arc::clone(&authority)]).unwrap_err(),
            MutationAdmission::Pending
        );
        drop(permit);
        let leases = try_reserve_all(&[Arc::clone(&authority)]).unwrap();
        for lease in leases {
            lease.commit();
        }
        assert_eq!(authority.snapshot_epoch(), RelationEpoch(EPOCH_STEP));
    });
}

#[test]
fn concurrent_mutations_do_not_lose_epoch_updates() {
    loom::model(|| {
        let authority = Arc::new(Authority::new(RelationEpoch(0)));
        let first_authority = Arc::clone(&authority);
        let first = loom::thread::spawn(move || loop {
            match try_reserve_all(&[Arc::clone(&first_authority)]) {
                Ok(leases) => {
                    for lease in leases {
                        lease.commit();
                    }
                    return;
                }
                Err(MutationAdmission::Pending) => loom::thread::yield_now(),
                Err(MutationAdmission::Exhausted) => panic!("関係世代が上限に達しました"),
            }
        });
        let second_authority = Arc::clone(&authority);
        let second = loom::thread::spawn(move || loop {
            match try_reserve_all(&[Arc::clone(&second_authority)]) {
                Ok(leases) => {
                    for lease in leases {
                        lease.commit();
                    }
                    return;
                }
                Err(MutationAdmission::Pending) => loom::thread::yield_now(),
                Err(MutationAdmission::Exhausted) => panic!("関係世代が上限に達しました"),
            }
        });
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(authority.snapshot_epoch(), RelationEpoch(EPOCH_STEP * 2));
    });
}

#[test]
fn multi_authority_pending_rolls_back_prior_reservation_without_epoch_drift() {
    loom::model(|| {
        let old_frontend = Arc::new(Authority::new(RelationEpoch(0)));
        let new_frontend = Arc::new(Authority::new(RelationEpoch(0)));
        let start = Authority::try_begin_start(&new_frontend, RelationEpoch(0)).unwrap();

        for _ in 0..2 {
            assert_eq!(
                try_reserve_all(&[Arc::clone(&old_frontend), Arc::clone(&new_frontend),])
                    .unwrap_err(),
                MutationAdmission::Pending
            );
            assert_eq!(old_frontend.snapshot_epoch(), RelationEpoch(0));
            assert_eq!(new_frontend.snapshot_epoch(), RelationEpoch(0));
        }

        drop(start);
        let leases =
            try_reserve_all(&[Arc::clone(&old_frontend), Arc::clone(&new_frontend)]).unwrap();
        for lease in leases {
            lease.commit();
        }
        assert_eq!(old_frontend.snapshot_epoch(), RelationEpoch(EPOCH_STEP));
        assert_eq!(new_frontend.snapshot_epoch(), RelationEpoch(EPOCH_STEP));
    });
}

#[test]
fn epoch_exhaustion_does_not_wrap_or_reuse_old_epoch() {
    loom::model(|| {
        let max_epoch = RelationEpoch(usize::MAX & !ACTIVE_MASK);
        let authority = Arc::new(Authority::new(max_epoch));
        assert_eq!(
            Authority::try_reserve_mutation(&authority).unwrap_err(),
            MutationAdmission::Exhausted
        );
        assert_eq!(authority.snapshot_epoch(), max_epoch);
    });
}
