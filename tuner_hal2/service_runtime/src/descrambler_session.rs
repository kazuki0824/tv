use crate::descrambler_key_table::{
    DescramblerKeyLookupError, DescramblerKeySlotId, DescramblerKeyTable,
};
use maleicacid_tuner_hal2_demux::PacketPid;
use maleicacid_tuner_hal2_descrambler::{
    DescramblerKeySlot, DescramblerKeyToken, DescramblerPid, DescramblerPidClaim,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DescramblerSession {
    demux_id: Option<i32>,
    demux_generation: Option<u64>,
    key_token: Option<DescramblerKeyToken>,
    key_slot: Option<DescramblerKeySlotId>,
    pid_claims: Vec<DescramblerPidClaim>,
    closed: bool,
}

impl DescramblerSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn demux_id(&self) -> Option<i32> {
        self.demux_id
    }
    pub(crate) fn demux_generation(&self) -> Option<u64> {
        self.demux_generation
    }
    pub(crate) fn key_slot(&self) -> Option<DescramblerKeySlotId> {
        self.key_slot
    }
    pub(crate) fn key_token(&self) -> Option<&DescramblerKeyToken> {
        self.key_token.as_ref()
    }
    pub(crate) fn pid_claims(&self) -> &[DescramblerPidClaim] {
        &self.pid_claims
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }
    pub(crate) fn clear_key(&mut self) -> Option<DescramblerKeyToken> {
        let old = self.key_token.take();
        self.key_slot = None;
        old
    }
    pub(crate) fn set_demux_binding(&mut self, demux_id: i32, generation: u64) {
        self.demux_id = Some(demux_id);
        self.demux_generation = Some(generation);
    }
    pub(crate) fn clear_pid_claims(&mut self) {
        self.pid_claims.clear();
    }
    pub(crate) fn set_key(&mut self, token: DescramblerKeyToken, key_slot: DescramblerKeySlotId) {
        self.key_token = Some(token);
        self.key_slot = Some(key_slot);
    }
    pub(crate) fn add_pid_claim(&mut self, claim: DescramblerPidClaim) {
        if !self.pid_claims.contains(&claim) {
            self.pid_claims.push(claim);
        }
    }
    pub(crate) fn remove_pid_claim(&mut self, claim: DescramblerPidClaim) {
        self.pid_claims.retain(|item| *item != claim);
    }
    pub(crate) fn close_all(&mut self) {
        self.pid_claims.clear();
        self.clear_key();
        self.demux_id = None;
        self.demux_generation = None;
        self.closed = true;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescramblerRuntimeResolvedClaimSet {
    claims: Vec<DescramblerPidClaim>,
    key_slot: Option<DescramblerKeySlot>,
}

impl DescramblerRuntimeResolvedClaimSet {
    fn new(claims: Vec<DescramblerPidClaim>, key_slot: Option<DescramblerKeySlot>) -> Self {
        Self { claims, key_slot }
    }

    pub(crate) fn into_parts(self) -> (Vec<DescramblerPidClaim>, Option<DescramblerKeySlot>) {
        (self.claims, self.key_slot)
    }
}

#[derive(Debug)]
pub(crate) struct DescramblerRuntime {
    session: DescramblerSession,
}

impl DescramblerRuntime {
    pub(crate) fn new() -> Self {
        Self {
            session: DescramblerSession::new(),
        }
    }
    pub(crate) fn demux_binding(&self) -> Option<(i32, u64)> {
        Some((self.session.demux_id()?, self.session.demux_generation()?))
    }

    pub(crate) fn is_bound_to_demux(&self, demux_id: i32, generation: u64) -> bool {
        !self.session.is_closed()
            && self.session.demux_id() == Some(demux_id)
            && self.session.demux_generation() == Some(generation)
    }

    pub(crate) fn is_bound_to_demux_id(&self, demux_id: i32) -> bool {
        !self.session.is_closed() && self.session.demux_id() == Some(demux_id)
    }

    pub(crate) fn has_pid_claim(&self, pid: DescramblerPid) -> bool {
        !self.session.is_closed()
            && self
                .session
                .pid_claims()
                .iter()
                .any(|claim| claim.pid() == pid)
    }

    pub(crate) fn has_stale_source_generation(
        &self,
        pid: DescramblerPid,
        source_filter_id: i32,
        source_generation: u64,
    ) -> bool {
        !self.session.is_closed()
            && self.session.pid_claims().iter().any(|stored| {
                let Some(source) = stored.source_filter_ref() else {
                    return false;
                };
                stored.pid() == pid
                    && source.filter_id() == source_filter_id
                    && source.generation() != source_generation
            })
    }

    pub(crate) fn resolved_claim_set_for_demux(
        &self,
        demux_id: i32,
        generation: u64,
        key_table: &DescramblerKeyTable,
    ) -> Option<DescramblerRuntimeResolvedClaimSet> {
        if !self.is_bound_to_demux(demux_id, generation) || self.session.pid_claims().is_empty() {
            return None;
        }
        let key_slot = self
            .session
            .key_slot()
            .and_then(|slot_id| key_table.key_slot(slot_id));
        Some(DescramblerRuntimeResolvedClaimSet::new(
            self.session.pid_claims().to_vec(),
            key_slot,
        ))
    }

    pub(crate) fn has_keyless_claim_for_demux_packet_pid(
        &self,
        demux_id: i32,
        generation: u64,
        packet_pid: PacketPid,
    ) -> bool {
        !self.has_key()
            && self.is_bound_to_demux(demux_id, generation)
            && self.session.pid_claims().iter().any(|claim| {
                PacketPid::from_descrambler_pid_for_service_runtime_boundary(claim.pid())
                    == packet_pid
            })
    }

    pub(crate) fn has_key(&self) -> bool {
        self.session.key_slot().is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_closed(&self) -> bool {
        self.session.is_closed()
    }

    pub(crate) fn bind_demux_use_case(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        bind_demux_use_case(&mut self.session, demux_id, demux_generation)
    }

    pub(crate) fn add_pid_claim_use_case(
        &mut self,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        add_pid_claim_use_case(&mut self.session, claim)
    }

    pub(crate) fn remove_pid_claim_use_case(
        &mut self,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        remove_pid_claim_use_case(&mut self.session, claim)
    }

    pub(crate) fn cleanup_all_use_case(
        &mut self,
        key_table: &mut DescramblerKeyTable,
    ) -> Result<DescramblerCleanupReport, DescramblerCleanupTxnError<DescramblerKeyLookupError>>
    {
        cleanup_all_use_case(&mut self.session, key_table)
    }

    pub(crate) fn clear_key_use_case(
        &mut self,
        key_table: &mut DescramblerKeyTable,
    ) -> Result<
        DescramblerClearKeyOutcome<DescramblerKeyLookupError>,
        DescramblerClearKeyTxnError,
    > {
        clear_key_use_case(&mut self.session, key_table)
    }

    pub(crate) fn replace_key_use_case(
        &mut self,
        key_table: &mut DescramblerKeyTable,
        token: DescramblerKeyToken,
    ) -> Result<
        DescramblerReplaceKeyOutcome<DescramblerKeyLookupError>,
        DescramblerReplaceKeyTxnError<DescramblerKeyLookupError, DescramblerKeyLookupError>,
    > {
        replace_key_use_case(&mut self.session, key_table, token)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DescramblerSessionTxnStep {
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
pub(crate) enum DescramblerSessionFailureKind {
    SessionClosed,
    DemuxNotBound,
    ClearKeyPlanMismatch,
    ReplaceKeyPlanMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DescramblerSessionFailure {
    pub step: DescramblerSessionTxnStep,
    pub kind: DescramblerSessionFailureKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescramblerCleanupReport {
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
    pub fn failure(&self) -> Option<DescramblerSessionFailure> {
        self.failed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DescramblerClearKeyPlan {
    NoKey,
    ClearExisting {
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlotId,
    },
}

impl DescramblerClearKeyPlan {
    fn old_token(&self) -> Option<&DescramblerKeyToken> {
        match self {
            Self::NoKey => None,
            Self::ClearExisting { token, .. } => Some(token),
        }
    }

    fn matches_session(&self, session: &DescramblerSession) -> bool {
        match self {
            Self::NoKey => session.key_token().is_none() && session.key_slot().is_none(),
            Self::ClearExisting { token, key_slot } => {
                session.key_token() == Some(token) && session.key_slot() == Some(*key_slot)
            }
        }
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

    fn requires_replace(&self) -> bool {
        matches!(self.inner, DescramblerReplaceKeyPlanKind::Replace { .. })
    }
}

#[derive(Debug, Default)]
pub(crate) struct DescramblerSessionTxn {
    steps: Vec<DescramblerSessionTxnStep>,
}

impl DescramblerSessionTxn {
    fn new() -> Self {
        Self::default()
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
        match (session.key_token().cloned(), session.key_slot()) {
            (None, None) => Ok(DescramblerClearKeyPlan::NoKey),
            (Some(token), Some(key_slot)) => Ok(DescramblerClearKeyPlan::ClearExisting {
                token,
                key_slot,
            }),
            _ => Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::CleanupKey,
                kind: DescramblerSessionFailureKind::ClearKeyPlanMismatch,
            }),
        }
    }

    fn validate_clear_key_plan(
        &mut self,
        session: &DescramblerSession,
        plan: &DescramblerClearKeyPlan,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        if !plan.matches_session(session) {
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

pub(crate) fn bind_demux_use_case(
    session: &mut DescramblerSession,
    demux_id: i32,
    generation: u64,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().bind_demux(session, demux_id, generation)
}

fn plan_replace_key_use_case(
    session: &DescramblerSession,
    token: &DescramblerKeyToken,
) -> Result<DescramblerReplaceKeyPlan, DescramblerSessionFailure> {
    DescramblerSessionTxn::new().plan_replace_key(session, token)
}

fn commit_replace_key_use_case(
    session: &mut DescramblerSession,
    plan: DescramblerReplaceKeyPlan,
    token: DescramblerKeyToken,
    key_slot: DescramblerKeySlotId,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().commit_validated_replace_key(session, plan, token, key_slot)
}

pub(crate) trait DescramblerKeyTxnOps {
    type LookupError;

    fn acquire_key_slot(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, Self::LookupError>;

    fn release_key_token(&mut self, token: &DescramblerKeyToken) -> Result<(), Self::LookupError>;
}

impl DescramblerKeyTxnOps for DescramblerKeyTable {
    type LookupError = DescramblerKeyLookupError;

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
pub(crate) enum DescramblerClearKeyTxnError {
    Session(DescramblerSessionFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DescramblerClearKeyOutcome<ReleaseError> {
    AlreadyClear,
    Cleared,
    ClearedWithOldKeyReleaseFailure {
        release_old: ReleaseError,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum DescramblerCleanupTxnError<ReleaseError> {
    Session(DescramblerSessionFailure),
    ReleaseKey(ReleaseError),
    ReleaseKeyAndSession {
        release: ReleaseError,
        session: DescramblerSessionFailure,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum DescramblerReplaceKeyTxnError<AcquireError, ReleaseError> {
    Session(DescramblerSessionFailure),
    Acquire(AcquireError),
    Commit {
        failure: DescramblerSessionFailure,
        rollback_release: Option<ReleaseError>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DescramblerReplaceKeyOutcome<ReleaseError> {
    AlreadyCurrent,
    Replaced,
    ReplacedWithOldKeyReleaseFailure {
        release_old: ReleaseError,
    },
}

pub(crate) fn clear_key_use_case<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
) -> Result<DescramblerClearKeyOutcome<KeyTable::LookupError>, DescramblerClearKeyTxnError>
where
    KeyTable: DescramblerKeyTxnOps,
{
    let prepared =
        prepare_clear_key_use_case(session).map_err(DescramblerClearKeyTxnError::Session)?;
    let old_token = prepared.plan.old_token().cloned();
    commit_prepared_clear_key_use_case(session, prepared)
        .map_err(DescramblerClearKeyTxnError::Session)?;
    let Some(token) = old_token.as_ref() else {
        return Ok(DescramblerClearKeyOutcome::AlreadyClear);
    };
    if let Err(release_old) = key_table.release_key_token(token) {
        return Ok(DescramblerClearKeyOutcome::ClearedWithOldKeyReleaseFailure {
            release_old,
        });
    }
    Ok(DescramblerClearKeyOutcome::Cleared)
}

pub(crate) fn replace_key_use_case<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
    token: DescramblerKeyToken,
) -> Result<
    DescramblerReplaceKeyOutcome<KeyTable::LookupError>,
    DescramblerReplaceKeyTxnError<KeyTable::LookupError, KeyTable::LookupError>,
>
where
    KeyTable: DescramblerKeyTxnOps,
{
    let plan = plan_replace_key_use_case(session, &token)
        .map_err(DescramblerReplaceKeyTxnError::Session)?;
    if !plan.requires_replace() {
        return Ok(DescramblerReplaceKeyOutcome::AlreadyCurrent);
    }
    let old_token = match &plan.inner {
        DescramblerReplaceKeyPlanKind::AlreadyCurrent { .. } => None,
        DescramblerReplaceKeyPlanKind::Replace { old_token } => old_token.clone(),
    };
    let key_slot = key_table
        .acquire_key_slot(&token)
        .map_err(DescramblerReplaceKeyTxnError::Acquire)?;
    if let Err(failure) = commit_replace_key_use_case(session, plan, token.clone(), key_slot) {
        let rollback_release = key_table.release_key_token(&token).err();
        return Err(DescramblerReplaceKeyTxnError::Commit {
            failure,
            rollback_release,
        });
    }
    if let Some(old_token) = old_token.as_ref() {
        if let Err(release_old) = key_table.release_key_token(old_token) {
            return Ok(DescramblerReplaceKeyOutcome::ReplacedWithOldKeyReleaseFailure {
                release_old,
            });
        }
    }
    Ok(DescramblerReplaceKeyOutcome::Replaced)
}

pub(crate) fn add_pid_claim_use_case(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().add_pid_claim(session, claim)
}

pub(crate) fn remove_pid_claim_use_case(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerSessionTxn::new().remove_pid_claim(session, claim)
}

fn prepare_clear_key_use_case(
    session: &DescramblerSession,
) -> Result<PreparedDescramblerClearKey, DescramblerSessionFailure> {
    let mut txn = DescramblerSessionTxn::new();
    let plan = txn.plan_clear_key(session)?;
    txn.validate_clear_key_plan(session, &plan)?;
    Ok(PreparedDescramblerClearKey::new(plan))
}

fn commit_prepared_clear_key_use_case(
    session: &mut DescramblerSession,
    prepared: PreparedDescramblerClearKey,
) -> Result<(), DescramblerSessionFailure> {
    let mut txn = DescramblerSessionTxn::new();
    txn.validate_clear_key_plan(session, &prepared.plan)?;
    txn.commit_validated_clear_key(session, prepared.plan);
    Ok(())
}

pub(crate) fn cleanup_all_use_case<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
) -> Result<DescramblerCleanupReport, DescramblerCleanupTxnError<KeyTable::LookupError>>
where
    KeyTable: DescramblerKeyTxnOps,
{
    let old_token = session.key_token().cloned();
    let release_error = old_token
        .as_ref()
        .and_then(|token| key_table.release_key_token(token).err());
    let report = DescramblerSessionTxn::new().cleanup_all(session);
    match (release_error, report.failure()) {
        (None, None) => Ok(report),
        (Some(error), None) => Err(DescramblerCleanupTxnError::ReleaseKey(error)),
        (None, Some(failure)) => Err(DescramblerCleanupTxnError::Session(failure)),
        (Some(error), Some(failure)) => Err(DescramblerCleanupTxnError::ReleaseKeyAndSession {
            release: error,
            session: failure,
        }),
    }
}
