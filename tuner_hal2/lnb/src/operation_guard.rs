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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LnbOperationGuard {
    pub lnb_id: i32,
    pub kind: LnbOperationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LnbOperationFailureRecord {
    pub lnb_id: i32,
    pub kind: LnbFailureKind,
    pub step: LnbFailureStep,
}

#[derive(Debug, Default)]
pub struct LnbOperationLedger {
    active: BTreeMap<i32, LnbOperationKind>,
    failures: BTreeMap<i32, LnbOperationFailureRecord>,
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
        self.active.insert(lnb_id, kind);
        Ok(LnbOperationGuard { lnb_id, kind })
    }

    pub fn finish(&mut self, guard: LnbOperationGuard) -> Result<(), LnbOperationFailureRecord> {
        match self.active.remove(&guard.lnb_id) {
            Some(active) if active == guard.kind => Ok(()),
            _ => {
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
        self.active.get(&lnb_id).copied()
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
}
