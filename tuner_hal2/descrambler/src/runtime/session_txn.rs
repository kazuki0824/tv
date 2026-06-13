use super::{
    DescramblerKeyLookupError, DescramblerKeySlotId, DescramblerKeyTable, DescramblerKeyToken,
    DescramblerPidClaim, DescramblerSession,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerSessionTxnStep {
    ValidateOpen,
    ValidateDemux,
    ResolveToken,
    ReplaceKey,
    AddPidClaim,
    RemovePidClaim,
    CleanupPidClaims,
    CleanupKey,
    CleanupDemuxBinding,
    Commit,
    Rollback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerSessionFailureKind {
    SessionClosed,
    DemuxNotBound,
    UnknownToken,
    ExpiredToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescramblerSessionFailure {
    pub step: DescramblerSessionTxnStep,
    pub kind: DescramblerSessionFailureKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescramblerCleanupReport {
    steps: Vec<DescramblerSessionTxnStep>,
    failed: Option<DescramblerSessionFailure>,
}

impl DescramblerCleanupReport {
    pub fn complete(steps: Vec<DescramblerSessionTxnStep>) -> Self {
        Self {
            steps,
            failed: None,
        }
    }
    pub fn failed(
        steps: Vec<DescramblerSessionTxnStep>,
        failure: DescramblerSessionFailure,
    ) -> Self {
        Self {
            steps,
            failed: Some(failure),
        }
    }
    pub fn steps(&self) -> &[DescramblerSessionTxnStep] {
        &self.steps
    }
    pub fn failure(&self) -> Option<DescramblerSessionFailure> {
        self.failed
    }
    pub fn is_complete(&self) -> bool {
        self.failed.is_none()
    }
}

#[derive(Debug, Default)]
pub struct DescramblerSessionTxn {
    steps: Vec<DescramblerSessionTxnStep>,
}

impl DescramblerSessionTxn {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn steps(&self) -> &[DescramblerSessionTxnStep] {
        &self.steps
    }

    fn record_step(&mut self, step: DescramblerSessionTxnStep) {
        self.steps.push(step);
    }

    fn ensure_open(
        &mut self,
        session: &DescramblerSession,
    ) -> Result<(), DescramblerSessionFailure> {
        self.record_step(DescramblerSessionTxnStep::ValidateOpen);
        if session.is_closed() {
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })
        } else {
            Ok(())
        }
    }

    pub fn bind_demux(
        &mut self,
        session: &mut DescramblerSession,
        demux_id: i32,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ValidateDemux);
        session.set_demux_id(demux_id);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    pub fn replace_key(
        &mut self,
        session: &mut DescramblerSession,
        key_table: &DescramblerKeyTable,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, DescramblerSessionFailure> {
        self.ensure_open(session)?;
        let snapshot = session.snapshot();
        self.record_step(DescramblerSessionTxnStep::ResolveToken);
        let key_slot = match key_table.resolve(token) {
            Ok(slot) => slot,
            Err(DescramblerKeyLookupError::UnknownToken) => {
                self.record_step(DescramblerSessionTxnStep::Rollback);
                session.restore(snapshot);
                return Err(DescramblerSessionFailure {
                    step: DescramblerSessionTxnStep::ResolveToken,
                    kind: DescramblerSessionFailureKind::UnknownToken,
                });
            }
            Err(DescramblerKeyLookupError::ExpiredToken) => {
                self.record_step(DescramblerSessionTxnStep::Rollback);
                session.restore(snapshot);
                return Err(DescramblerSessionFailure {
                    step: DescramblerSessionTxnStep::ResolveToken,
                    kind: DescramblerSessionFailureKind::ExpiredToken,
                });
            }
        };
        self.record_step(DescramblerSessionTxnStep::ReplaceKey);
        session.set_key_slot(key_slot);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(key_slot)
    }

    pub fn add_pid_claim(
        &mut self,
        session: &mut DescramblerSession,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ValidateDemux);
        if session.demux_id().is_none() {
            return Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateDemux,
                kind: DescramblerSessionFailureKind::DemuxNotBound,
            });
        }
        self.record_step(DescramblerSessionTxnStep::AddPidClaim);
        session.add_pid_claim(claim);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    pub fn remove_pid_claim(
        &mut self,
        session: &mut DescramblerSession,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::RemovePidClaim);
        session.remove_pid_claim(claim);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    pub fn cleanup_all(&mut self, session: &mut DescramblerSession) -> DescramblerCleanupReport {
        self.record_step(DescramblerSessionTxnStep::CleanupPidClaims);
        self.record_step(DescramblerSessionTxnStep::CleanupKey);
        self.record_step(DescramblerSessionTxnStep::CleanupDemuxBinding);
        session.close_all();
        self.record_step(DescramblerSessionTxnStep::Commit);
        DescramblerCleanupReport::complete(self.steps.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{DescramblerKeySlotId, DescramblerPidClaim, DescramblerSession};

    #[test]
    fn replace_key_uses_token_table_and_does_not_store_token_bytes_in_session() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1, 2, 3]).unwrap();
        let mut table = DescramblerKeyTable::default();
        table.insert_test_key(token.clone(), DescramblerKeySlotId(44));
        let mut session = DescramblerSession::new();
        let mut txn = DescramblerSessionTxn::new();
        assert_eq!(
            txn.replace_key(&mut session, &table, &token),
            Ok(DescramblerKeySlotId(44))
        );
        assert_eq!(session.key_slot(), Some(DescramblerKeySlotId(44)));
        assert!(txn
            .steps()
            .contains(&DescramblerSessionTxnStep::ResolveToken));
        assert!(txn.steps().contains(&DescramblerSessionTxnStep::ReplaceKey));
    }

    #[test]
    fn unknown_key_rolls_back_existing_key_slot() {
        let known = DescramblerKeyToken::try_from_bytes(vec![1]).unwrap();
        let unknown = DescramblerKeyToken::try_from_bytes(vec![2]).unwrap();
        let mut table = DescramblerKeyTable::default();
        table.insert_test_key(known.clone(), DescramblerKeySlotId(7));
        let mut session = DescramblerSession::new();
        let mut txn = DescramblerSessionTxn::new();
        assert_eq!(
            txn.replace_key(&mut session, &table, &known),
            Ok(DescramblerKeySlotId(7))
        );
        let mut failed = DescramblerSessionTxn::new();
        assert_eq!(
            failed.replace_key(&mut session, &table, &unknown),
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ResolveToken,
                kind: DescramblerSessionFailureKind::UnknownToken
            })
        );
        assert_eq!(session.key_slot(), Some(DescramblerKeySlotId(7)));
        assert!(failed
            .steps()
            .contains(&DescramblerSessionTxnStep::Rollback));
    }

    #[test]
    fn pid_claim_requires_bound_demux_and_keeps_source_generation() {
        let claim = DescramblerPidClaim::from_source_filter(200, 3, 11).unwrap();
        let mut session = DescramblerSession::new();
        let mut missing_demux = DescramblerSessionTxn::new();
        assert_eq!(
            missing_demux.add_pid_claim(&mut session, claim),
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateDemux,
                kind: DescramblerSessionFailureKind::DemuxNotBound
            })
        );
        let mut bind = DescramblerSessionTxn::new();
        assert_eq!(bind.bind_demux(&mut session, 8), Ok(()));
        let mut add = DescramblerSessionTxn::new();
        assert_eq!(add.add_pid_claim(&mut session, claim), Ok(()));
        assert_eq!(session.pid_claims(), &[claim]);
    }

    #[test]
    fn cleanup_closes_demux_key_and_pid_claims_as_one_transaction() {
        let token = DescramblerKeyToken::try_from_bytes(vec![9]).unwrap();
        let mut table = DescramblerKeyTable::default();
        table.insert_test_key(token.clone(), DescramblerKeySlotId(9));
        let claim = DescramblerPidClaim::from_source_filter(100, 2, 4).unwrap();
        let mut session = DescramblerSession::new();
        let mut prepare = DescramblerSessionTxn::new();
        assert_eq!(prepare.bind_demux(&mut session, 1), Ok(()));
        assert_eq!(
            prepare.replace_key(&mut session, &table, &token),
            Ok(DescramblerKeySlotId(9))
        );
        assert_eq!(prepare.add_pid_claim(&mut session, claim), Ok(()));
        let mut cleanup = DescramblerSessionTxn::new();
        let report = cleanup.cleanup_all(&mut session);
        assert!(report.is_complete());
        assert_eq!(session.demux_id(), None);
        assert_eq!(session.key_slot(), None);
        assert!(session.pid_claims().is_empty());
        assert!(session.is_closed());
        assert_eq!(
            report.steps(),
            &[
                DescramblerSessionTxnStep::CleanupPidClaims,
                DescramblerSessionTxnStep::CleanupKey,
                DescramblerSessionTxnStep::CleanupDemuxBinding,
                DescramblerSessionTxnStep::Commit,
            ]
        );
    }
}
