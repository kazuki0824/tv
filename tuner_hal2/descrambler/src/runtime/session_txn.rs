use super::{DescramblerKeySlotId, DescramblerKeyToken, DescramblerPidClaim, DescramblerSession};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerSessionTxnStep {
    ValidateOpen,
    ValidateDemux,
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
    ClearKeyPlanMismatch,
    ReplaceKeyPlanMismatch,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerClearKeyPlan {
    old_token: Option<DescramblerKeyToken>,
    old_key_slot: Option<DescramblerKeySlotId>,
}

impl DescramblerClearKeyPlan {
    fn old_token(&self) -> Option<&DescramblerKeyToken> {
        self.old_token.as_ref()
    }
    fn old_key_slot(&self) -> Option<DescramblerKeySlotId> {
        self.old_key_slot
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedDescramblerClearKey {
    plan: DescramblerClearKeyPlan,
}

impl PreparedDescramblerClearKey {
    fn new(plan: DescramblerClearKeyPlan) -> Self {
        Self { plan }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerReplaceKeyPlan {
    inner: DescramblerReplaceKeyPlanKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DescramblerReplaceKeyPlanKind {
    AlreadyCurrent {
        expected_token: DescramblerKeyToken,
    },
    Replace {
        old_token: Option<DescramblerKeyToken>,
    },
}

impl DescramblerReplaceKeyPlan {
    fn already_current(expected_token: DescramblerKeyToken) -> Self {
        Self {
            inner: DescramblerReplaceKeyPlanKind::AlreadyCurrent { expected_token },
        }
    }

    fn replace(old_token: Option<DescramblerKeyToken>) -> Self {
        Self {
            inner: DescramblerReplaceKeyPlanKind::Replace { old_token },
        }
    }

    fn old_token(&self) -> Option<&DescramblerKeyToken> {
        match &self.inner {
            DescramblerReplaceKeyPlanKind::AlreadyCurrent { .. } => None,
            DescramblerReplaceKeyPlanKind::Replace { old_token } => old_token.as_ref(),
        }
    }

    fn requires_replace(&self) -> bool {
        matches!(self.inner, DescramblerReplaceKeyPlanKind::Replace { .. })
    }
}

#[derive(Debug, Default)]
pub struct DescramblerSessionTxn {
    steps: Vec<DescramblerSessionTxnStep>,
}

impl DescramblerSessionTxn {
    fn new() -> Self {
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

    fn bind_demux(
        &mut self,
        session: &mut DescramblerSession,
        demux_id: i32,
        generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ValidateDemux);
        if session.demux_id() != Some(demux_id) || session.demux_generation() != Some(generation) {
            self.record_step(DescramblerSessionTxnStep::CleanupPidClaims);
            session.clear_pid_claims();
        }
        session.set_demux_binding(demux_id, generation);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    fn plan_replace_key(
        &mut self,
        session: &DescramblerSession,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerReplaceKeyPlan, DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ReplaceKey);
        if session.key_token() == Some(token) {
            return Ok(DescramblerReplaceKeyPlan::already_current(token.clone()));
        }
        Ok(DescramblerReplaceKeyPlan::replace(
            session.key_token().cloned(),
        ))
    }

    fn commit_validated_replace_key(
        &mut self,
        session: &mut DescramblerSession,
        plan: DescramblerReplaceKeyPlan,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlotId,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        match plan.inner {
            DescramblerReplaceKeyPlanKind::AlreadyCurrent { expected_token } => {
                if session.key_token() == Some(&expected_token) {
                    Ok(())
                } else {
                    self.record_step(DescramblerSessionTxnStep::Rollback);
                    Err(DescramblerSessionFailure {
                        step: DescramblerSessionTxnStep::ReplaceKey,
                        kind: DescramblerSessionFailureKind::ReplaceKeyPlanMismatch,
                    })
                }
            }
            DescramblerReplaceKeyPlanKind::Replace { old_token } => {
                if session.key_token() != old_token.as_ref() {
                    self.record_step(DescramblerSessionTxnStep::Rollback);
                    return Err(DescramblerSessionFailure {
                        step: DescramblerSessionTxnStep::ReplaceKey,
                        kind: DescramblerSessionFailureKind::ReplaceKeyPlanMismatch,
                    });
                }
                session.set_key(token, key_slot);
                self.record_step(DescramblerSessionTxnStep::Commit);
                Ok(())
            }
        }
    }

    #[cfg(test)]
    pub fn replace_key(
        &mut self,
        session: &mut DescramblerSession,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlotId,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ReplaceKey);
        session.set_key(token, key_slot);
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    fn add_pid_claim(
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

    fn remove_pid_claim(
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

    fn plan_clear_key(
        &mut self,
        session: &DescramblerSession,
    ) -> Result<DescramblerClearKeyPlan, DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::CleanupKey);
        Ok(DescramblerClearKeyPlan {
            old_token: session.key_token().cloned(),
            old_key_slot: session.key_slot(),
        })
    }

    fn validate_clear_key_plan(
        &mut self,
        session: &DescramblerSession,
        plan: &DescramblerClearKeyPlan,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        if session.key_token() != plan.old_token() || session.key_slot() != plan.old_key_slot() {
            self.record_step(DescramblerSessionTxnStep::Rollback);
            return Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::CleanupKey,
                kind: DescramblerSessionFailureKind::ClearKeyPlanMismatch,
            });
        }
        Ok(())
    }

    fn commit_validated_clear_key(
        &mut self,
        session: &mut DescramblerSession,
        _plan: DescramblerClearKeyPlan,
    ) {
        session.clear_key();
        self.record_step(DescramblerSessionTxnStep::Commit);
    }

    fn cleanup_all(&mut self, session: &mut DescramblerSession) -> DescramblerCleanupReport {
        self.record_step(DescramblerSessionTxnStep::CleanupPidClaims);
        self.record_step(DescramblerSessionTxnStep::CleanupKey);
        self.record_step(DescramblerSessionTxnStep::CleanupDemuxBinding);
        session.close_all();
        self.record_step(DescramblerSessionTxnStep::Commit);
        DescramblerCleanupReport::complete(self.steps.clone())
    }
}

pub fn bind_demux_with_session_txn(
    session: &mut DescramblerSession,
    demux_id: i32,
    generation: u64,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().bind_demux(session, demux_id, generation)
}

fn plan_replace_key_with_session_txn(
    session: &DescramblerSession,
    token: &DescramblerKeyToken,
) -> Result<DescramblerReplaceKeyPlan, DescramblerSessionFailure> {
    DescramblerSessionTxn::new().plan_replace_key(session, token)
}

fn commit_replace_key_with_session_txn(
    session: &mut DescramblerSession,
    plan: DescramblerReplaceKeyPlan,
    token: DescramblerKeyToken,
    key_slot: DescramblerKeySlotId,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().commit_validated_replace_key(session, plan, token, key_slot)
}

pub trait DescramblerKeyTxnOps {
    type LookupError;

    fn acquire_key_slot(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, Self::LookupError>;

    fn release_key_token(&mut self, token: &DescramblerKeyToken) -> Result<(), Self::LookupError>;
}

impl DescramblerKeyTxnOps for super::DescramblerKeyTable {
    type LookupError = super::DescramblerKeyLookupError;

    fn acquire_key_slot(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, Self::LookupError> {
        self.acquire(token)
    }

    fn release_key_token(&mut self, token: &DescramblerKeyToken) -> Result<(), Self::LookupError> {
        self.release(token)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DescramblerClearKeyTxnError<ReleaseError> {
    Session(DescramblerSessionFailure),
    ReleaseOld(ReleaseError),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DescramblerReplaceKeyTxnError<AcquireError, ReleaseError> {
    Session(DescramblerSessionFailure),
    Acquire(AcquireError),
    Commit {
        failure: DescramblerSessionFailure,
        rollback_release: Option<ReleaseError>,
    },
    ReleaseOld(ReleaseError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerReplaceKeyOutcome {
    AlreadyCurrent,
    Replaced,
}

pub fn clear_key_with_session_txn<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
) -> Result<(), DescramblerClearKeyTxnError<KeyTable::LookupError>>
where
    KeyTable: DescramblerKeyTxnOps,
{
    let prepared = prepare_clear_key_with_session_txn(session)
        .map_err(DescramblerClearKeyTxnError::Session)?;
    let old_token = prepared.plan.old_token.clone();
    commit_prepared_clear_key_with_session_txn(session, prepared)
        .map_err(DescramblerClearKeyTxnError::Session)?;
    if let Some(token) = old_token.as_ref() {
        key_table
            .release_key_token(token)
            .map_err(DescramblerClearKeyTxnError::ReleaseOld)?;
    }
    Ok(())
}

pub fn replace_key_with_session_txn<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
    token: DescramblerKeyToken,
) -> Result<
    DescramblerReplaceKeyOutcome,
    DescramblerReplaceKeyTxnError<KeyTable::LookupError, KeyTable::LookupError>,
>
where
    KeyTable: DescramblerKeyTxnOps,
{
    let plan = plan_replace_key_with_session_txn(session, &token)
        .map_err(DescramblerReplaceKeyTxnError::Session)?;
    if !plan.requires_replace() {
        return Ok(DescramblerReplaceKeyOutcome::AlreadyCurrent);
    }
    let old_token = plan.old_token().cloned();
    let key_slot = key_table
        .acquire_key_slot(&token)
        .map_err(DescramblerReplaceKeyTxnError::Acquire)?;
    if let Err(failure) =
        commit_replace_key_with_session_txn(session, plan, token.clone(), key_slot)
    {
        let rollback_release = key_table.release_key_token(&token).err();
        return Err(DescramblerReplaceKeyTxnError::Commit {
            failure,
            rollback_release,
        });
    }
    if let Some(old_token) = old_token.as_ref() {
        key_table
            .release_key_token(old_token)
            .map_err(DescramblerReplaceKeyTxnError::ReleaseOld)?;
    }
    Ok(DescramblerReplaceKeyOutcome::Replaced)
}

pub fn add_pid_claim_with_session_txn(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().add_pid_claim(session, claim)
}

pub fn remove_pid_claim_with_session_txn(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().remove_pid_claim(session, claim)
}

fn prepare_clear_key_with_session_txn(
    session: &DescramblerSession,
) -> Result<PreparedDescramblerClearKey, DescramblerSessionFailure> {
    let mut txn = DescramblerSessionTxn::new();
    let plan = txn.plan_clear_key(session)?;
    txn.validate_clear_key_plan(session, &plan)?;
    Ok(PreparedDescramblerClearKey::new(plan))
}

fn commit_prepared_clear_key_with_session_txn(
    session: &mut DescramblerSession,
    prepared: PreparedDescramblerClearKey,
) -> Result<(), DescramblerSessionFailure> {
    let mut txn = DescramblerSessionTxn::new();
    txn.validate_clear_key_plan(session, &prepared.plan)?;
    txn.commit_validated_clear_key(session, prepared.plan);
    Ok(())
}

pub fn cleanup_all_with_session_txn(session: &mut DescramblerSession) -> DescramblerCleanupReport {
    DescramblerSessionTxn::new().cleanup_all(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{DescramblerKeySlotId, DescramblerPidClaim, DescramblerSession};

    #[test]
    fn replace_key_records_session_replace_transaction() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1; 8]).unwrap();
        let mut session = DescramblerSession::new();
        let mut txn = DescramblerSessionTxn::new();
        assert_eq!(
            txn.replace_key(&mut session, token.clone(), DescramblerKeySlotId(44)),
            Ok(())
        );
        assert_eq!(session.key_slot(), Some(DescramblerKeySlotId(44)));
        assert_eq!(session.key_token(), Some(&token));
        assert!(txn.steps().contains(&DescramblerSessionTxnStep::ReplaceKey));
        assert!(txn.steps().contains(&DescramblerSessionTxnStep::Commit));
    }

    #[test]
    fn replace_key_rejects_closed_session_without_mutating() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1; 8]).unwrap();
        let new_token = DescramblerKeyToken::try_from_bytes(vec![2; 8]).unwrap();
        let mut session = DescramblerSession::new();
        let mut prepare = DescramblerSessionTxn::new();
        prepare
            .replace_key(&mut session, token.clone(), DescramblerKeySlotId(7))
            .unwrap();
        session.close_all();
        let mut failed = DescramblerSessionTxn::new();
        assert_eq!(
            failed.replace_key(&mut session, new_token, DescramblerKeySlotId(8)),
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed
            })
        );
        assert_eq!(session.key_slot(), None);
        assert_eq!(session.key_token(), None);
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
        assert_eq!(bind.bind_demux(&mut session, 8, 14), Ok(()));
        let mut add = DescramblerSessionTxn::new();
        assert_eq!(add.add_pid_claim(&mut session, claim), Ok(()));
        assert_eq!(session.demux_generation(), Some(14));
        assert_eq!(session.pid_claims(), &[claim]);
    }

    #[test]
    fn cleanup_closes_demux_key_and_pid_claims_as_one_transaction() {
        let token = DescramblerKeyToken::try_from_bytes(vec![9; 8]).unwrap();
        let claim = DescramblerPidClaim::from_source_filter(100, 2, 4).unwrap();
        let mut session = DescramblerSession::new();
        let mut prepare = DescramblerSessionTxn::new();
        assert_eq!(prepare.bind_demux(&mut session, 1, 2), Ok(()));
        assert_eq!(
            prepare.replace_key(&mut session, token.clone(), DescramblerKeySlotId(9)),
            Ok(())
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

    #[test]
    fn clear_key_keeps_demux_and_pid_claims() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1; 8]).unwrap();
        let claim = DescramblerPidClaim::from_source_filter(200, 5, 8).unwrap();
        let mut session = DescramblerSession::new();
        let mut bind = DescramblerSessionTxn::new();
        bind.bind_demux(&mut session, 11, 12).unwrap();
        let mut replace = DescramblerSessionTxn::new();
        replace
            .replace_key(&mut session, token.clone(), DescramblerKeySlotId(4))
            .unwrap();
        let mut add = DescramblerSessionTxn::new();
        add.add_pid_claim(&mut session, claim).unwrap();

        let mut clear = DescramblerSessionTxn::new();
        let plan = clear.plan_clear_key(&session).unwrap();
        assert_eq!(plan.old_token(), Some(&token));
        assert_eq!(session.key_slot(), Some(DescramblerKeySlotId(4)));
        clear.commit_validated_clear_key(&mut session, plan);
        assert_eq!(session.demux_id(), Some(11));
        assert_eq!(session.demux_generation(), Some(12));
        assert_eq!(session.pid_claims(), &[claim]);
        assert_eq!(session.key_slot(), None);
        assert_eq!(session.key_token(), None);
        assert!(!session.is_closed());
    }

    #[test]
    fn validate_clear_key_plan_rejects_stale_plan_without_mutating_session() {
        let old_token = DescramblerKeyToken::try_from_bytes(vec![5; 8]).unwrap();
        let new_token = DescramblerKeyToken::try_from_bytes(vec![6; 8]).unwrap();
        let mut session = DescramblerSession::new();

        let mut old_key = DescramblerSessionTxn::new();
        old_key
            .replace_key(&mut session, old_token.clone(), DescramblerKeySlotId(4))
            .unwrap();
        let mut clear = DescramblerSessionTxn::new();
        let stale_plan = clear.plan_clear_key(&session).unwrap();

        let mut replace = DescramblerSessionTxn::new();
        replace
            .replace_key(&mut session, new_token.clone(), DescramblerKeySlotId(5))
            .unwrap();

        assert_eq!(
            clear.validate_clear_key_plan(&session, &stale_plan),
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::CleanupKey,
                kind: DescramblerSessionFailureKind::ClearKeyPlanMismatch,
            })
        );
        assert_eq!(session.key_token(), Some(&new_token));
        assert_eq!(session.key_slot(), Some(DescramblerKeySlotId(5)));
        assert!(clear.steps().contains(&DescramblerSessionTxnStep::Rollback));
    }
}
