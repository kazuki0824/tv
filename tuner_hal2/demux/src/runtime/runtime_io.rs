use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RuntimeIoKind {
    Filter,
    Dvr,
    Av,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeIoFailureKind {
    QueueClearFailed,
    DeliveryFailed,
    EventFlagWakeFailed,
    SharedBackingFailed,
    WorkerFailed,
}

#[derive(Debug, Default)]
pub struct RuntimeIoRegistry {
    failures: BTreeMap<(RuntimeIoKind, i32), RuntimeIoFailureKind>,
}

impl RuntimeIoRegistry {
    pub fn mark_failed(&mut self, kind: RuntimeIoKind, id: i32, failure: RuntimeIoFailureKind) {
        self.failures.insert((kind, id), failure);
    }
    pub fn failure(&self, kind: RuntimeIoKind, id: i32) -> Option<RuntimeIoFailureKind> {
        self.failures.get(&(kind, id)).copied()
    }
}
