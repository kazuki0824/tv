use std::sync::atomic::AtomicU64;

use binder::{Result as BinderResult, Status};

use crate::tuner_hal::record_tuner_diagnostic_counter;

#[derive(Debug, Clone, Copy)]
pub(crate) struct QueueCleanupTxn {
    owner_kind: &'static str,
    owner_id: i32,
    phase: &'static str,
}

impl QueueCleanupTxn {
    pub(crate) fn new(owner_kind: &'static str, owner_id: i32, phase: &'static str) -> Self {
        Self { owner_kind, owner_id, phase }
    }

    fn record_failure(&self, step: &'static str, counter: &AtomicU64, status: &Status) {
        record_tuner_diagnostic_counter(counter, step);
        eprintln!(
            "maleicacid-tuner-hal-queue-cleanup: owner={} id={} phase={} step={} status={:?}",
            self.owner_kind, self.owner_id, self.phase, step, status
        );
    }

    pub(crate) fn best_effort<F>(&self, step: &'static str, counter: &AtomicU64, f: F)
    where
        F: FnOnce() -> BinderResult<()>,
    {
        if let Err(status) = f() {
            self.record_failure(step, counter, &status);
        }
    }

    pub(crate) fn required<F>(&self, step: &'static str, counter: &AtomicU64, f: F) -> BinderResult<()>
    where
        F: FnOnce() -> BinderResult<()>,
    {
        match f() {
            Ok(()) => Ok(()),
            Err(status) => {
                self.record_failure(step, counter, &status);
                Err(status)
            }
        }
    }

    pub(crate) fn required_resource<R>(
        &self,
        owner: &R,
        resource: &'static str,
        step: &'static str,
        counter: &AtomicU64,
    ) -> BinderResult<()>
    where
        R: QueueCleanupResource + ?Sized,
    {
        self.required(step, counter, || owner.cleanup_queue_resource(resource, self.phase))
    }
}

pub(crate) trait QueueCleanupResource {
    fn cleanup_queue_resource(&self, resource: &'static str, reason: &'static str) -> BinderResult<()>;
}
