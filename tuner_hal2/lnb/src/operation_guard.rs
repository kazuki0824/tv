use std::collections::BTreeMap;

use crate::{LnbFailureKind, LnbFailureStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbOperationKind {
    Voltage,
    Tone,
    SatellitePosition,
    Diseqc,
    Close,
}

#[derive(Debug, Eq, PartialEq)]
pub struct LnbOperationGuard {
    lnb_id: i32,
    kind: LnbOperationKind,
    nonce: u64,
}

impl LnbOperationGuard {
    pub const fn lnb_id(&self) -> i32 {
        self.lnb_id
    }

    pub const fn kind(&self) -> LnbOperationKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LnbOperationFailureRecord {
    pub lnb_id: i32,
    pub kind: LnbFailureKind,
    pub step: LnbFailureStep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveLnbOperation {
    kind: LnbOperationKind,
    nonce: u64,
}

#[derive(Debug, Default)]
pub struct LnbOperationLedger {
    active: BTreeMap<i32, ActiveLnbOperation>,
    failures: BTreeMap<i32, LnbOperationFailureRecord>,
    next_nonce: u64,
}

impl LnbOperationLedger {
    pub fn begin(
        &mut self,
        lnb_id: i32,
        kind: LnbOperationKind,
    ) -> Result<LnbOperationGuard, LnbOperationFailureRecord> {
        if self.active.contains_key(&lnb_id) {
            let record = LnbOperationFailureRecord {
                lnb_id,
                kind: LnbFailureKind::OperationAlreadyActive,
                step: LnbFailureStep::ValidateState,
            };
            self.failures.insert(lnb_id, record);
            return Err(record);
        }
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.wrapping_add(1);
        self.active
            .insert(lnb_id, ActiveLnbOperation { kind, nonce });
        Ok(LnbOperationGuard {
            lnb_id,
            kind,
            nonce,
        })
    }

    pub fn finish(&mut self, guard: LnbOperationGuard) -> Result<(), LnbOperationFailureRecord> {
        match self.active.remove(&guard.lnb_id) {
            Some(active) if active.kind == guard.kind && active.nonce == guard.nonce => Ok(()),
            Some(active) => {
                self.active.insert(guard.lnb_id, active);
                let record = LnbOperationFailureRecord {
                    lnb_id: guard.lnb_id,
                    kind: LnbFailureKind::OperationLockFailed,
                    step: LnbFailureStep::CommitClosed,
                };
                self.failures.insert(guard.lnb_id, record);
                Err(record)
            }
            None => {
                let record = LnbOperationFailureRecord {
                    lnb_id: guard.lnb_id,
                    kind: LnbFailureKind::OperationLockFailed,
                    step: LnbFailureStep::CommitClosed,
                };
                self.failures.insert(guard.lnb_id, record);
                Err(record)
            }
        }
    }

    pub fn active_operation(&self, lnb_id: i32) -> Option<LnbOperationKind> {
        self.active.get(&lnb_id).map(|operation| operation.kind)
    }

    pub fn failure(&self, lnb_id: i32) -> Option<LnbOperationFailureRecord> {
        self.failures.get(&lnb_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_ledger_rejects_duplicate_active_operation() {
        let mut ledger = LnbOperationLedger::default();
        let _guard = ledger.begin(4, LnbOperationKind::Voltage).unwrap();
        let err = ledger.begin(4, LnbOperationKind::Close).unwrap_err();
        assert_eq!(err.kind, LnbFailureKind::OperationAlreadyActive);
        assert_eq!(ledger.active_operation(4), Some(LnbOperationKind::Voltage));
    }

    #[test]
    fn operation_ledger_finish_clears_active_operation() {
        let mut ledger = LnbOperationLedger::default();
        let guard = ledger.begin(5, LnbOperationKind::Tone).unwrap();
        ledger.finish(guard).unwrap();
        assert_eq!(ledger.active_operation(5), None);
    }

    #[test]
    fn operation_guard_is_consumed_by_finish() {
        let mut ledger = LnbOperationLedger::default();
        let guard = ledger.begin(6, LnbOperationKind::Voltage).unwrap();
        assert_eq!(guard.lnb_id(), 6);
        assert_eq!(guard.kind(), LnbOperationKind::Voltage);
        ledger.finish(guard).unwrap();
        assert_eq!(ledger.active_operation(6), None);
    }
}
