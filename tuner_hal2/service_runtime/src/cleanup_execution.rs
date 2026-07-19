use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

use crate::diagnostics::{BoundedDiagnosticStore, DEFAULT_DIAGNOSTIC_STORE_LIMIT};

pub trait CleanupExecutionStepOutcome {
    type Failure: Clone;

    fn result(&self) -> Result<(), Self::Failure>;

    fn into_result(self) -> Result<(), Self::Failure>;
}

#[derive(Clone, Debug)]
pub struct CleanupExecutionReport<TStepOutcome, TFailure> {
    outcomes: Vec<TStepOutcome>,
    _failure: PhantomData<fn() -> TFailure>,
}

impl<TStepOutcome, TFailure> Default for CleanupExecutionReport<TStepOutcome, TFailure> {
    fn default() -> Self {
        Self::new()
    }
}

impl<TStepOutcome, TFailure> CleanupExecutionReport<TStepOutcome, TFailure> {
    pub fn new() -> Self {
        Self {
            outcomes: Vec::new(),
            _failure: PhantomData,
        }
    }

    pub fn push(&mut self, outcome: TStepOutcome) {
        self.outcomes.push(outcome);
    }

    pub fn outcomes(&self) -> &[TStepOutcome] {
        &self.outcomes
    }

    pub fn extend(&mut self, other: Self) {
        self.outcomes.extend(other.outcomes);
    }
}

impl<TStepOutcome, TFailure> CleanupExecutionReport<TStepOutcome, TFailure>
where
    TStepOutcome: CleanupExecutionStepOutcome<Failure = TFailure>,
    TFailure: Clone,
{
    pub fn first_error(&self) -> Option<TFailure> {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.result().err())
            .next()
    }

    pub fn result(&self) -> Result<(), TFailure> {
        self.first_error().map_or(Ok(()), Err)
    }

    pub fn into_result(self) -> Result<(), TFailure> {
        match self
            .outcomes
            .into_iter()
            .find_map(|outcome| outcome.into_result().err())
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CleanupExecutionDiagnosticSnapshot<TRecord> {
    records: Vec<TRecord>,
    dropped_count: u64,
    record_failure_count: u64,
}

impl<TRecord> CleanupExecutionDiagnosticSnapshot<TRecord> {
    pub fn new(records: Vec<TRecord>, dropped_count: u64) -> Self {
        Self::new_with_record_failure_count(records, dropped_count, 0)
    }

    pub fn new_with_record_failure_count(
        records: Vec<TRecord>,
        dropped_count: u64,
        record_failure_count: u64,
    ) -> Self {
        Self {
            records,
            dropped_count,
            record_failure_count,
        }
    }

    pub fn records(&self) -> &[TRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn record_failure_count(&self) -> u64 {
        self.record_failure_count
    }
}

#[derive(Clone, Debug)]
pub struct SharedCleanupDiagnostics<TRecord> {
    records: Arc<Mutex<BoundedDiagnosticStore<TRecord>>>,
    record_failure_count: Arc<AtomicU64>,
}

fn saturating_increment_atomic_u64(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

impl<TRecord> SharedCleanupDiagnostics<TRecord> {
    pub fn new(limit: usize) -> Self {
        Self {
            records: Arc::new(Mutex::new(BoundedDiagnosticStore::new(limit))),
            record_failure_count: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl<TRecord> SharedCleanupDiagnostics<TRecord>
where
    TRecord: Clone,
{
    pub fn record(&self, record: TRecord) -> Result<(), HalError> {
        let mut records = match self.records.lock() {
            Ok(records) => records,
            Err(_) => {
                saturating_increment_atomic_u64(&self.record_failure_count);
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "cleanup diagnostic store lock poisoned",
                ));
            }
        };
        records.push(record);
        Ok(())
    }

    pub fn record_nonblocking(&self, record: TRecord) {
        // The failure counter is updated by `record`; lifecycle transitions must not be
        // aborted solely because the bounded diagnostic store is unavailable.
        let _record_result = self.record(record);
    }

    pub fn snapshot(&self) -> Result<CleanupExecutionDiagnosticSnapshot<TRecord>, HalError> {
        let records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup diagnostic store lock poisoned while snapshotting",
            )
        })?;
        Ok(
            CleanupExecutionDiagnosticSnapshot::new_with_record_failure_count(
                records.as_slice().to_vec(),
                records.dropped_count(),
                self.record_failure_count.load(Ordering::Relaxed),
            ),
        )
    }

    pub fn clear(&self) -> Result<(), HalError> {
        let mut records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup diagnostic store lock poisoned while clearing",
            )
        })?;
        records.clear();
        self.record_failure_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}

impl<TRecord> Default for SharedCleanupDiagnostics<TRecord> {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_STORE_LIMIT)
    }
}
