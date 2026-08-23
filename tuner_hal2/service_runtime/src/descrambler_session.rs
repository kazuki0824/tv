use crate::descrambler_key_table::{
    DescramblerKeyLookupError, DescramblerKeySlotId, DescramblerKeyTable,
};
use maleicacid_tuner_hal2_demux::PacketPid;
use maleicacid_tuner_hal2_descrambler::{
    DescramblerKeySlot, DescramblerKeyToken, DescramblerPid, DescramblerPidClaim,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DescramblerSessionState {
    #[default]
    Open,
    Closed,
    CleanupPending,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DescramblerSourceCallState {
    #[default]
    NeverCalledUnbound,
    CallConsumedUnbound {
        failure: Option<DescramblerSourceCallFailure>,
    },
    Bound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DescramblerSourceCallFailure {
    InvalidDemuxId,
    InvalidDemuxState,
    BindingCommitFailed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DescramblerSession {
    demux_id: Option<i32>,
    demux_generation: Option<u64>,
    source_call_state: DescramblerSourceCallState,
    key_token: Option<DescramblerKeyToken>,
    key_slot: Option<DescramblerKeySlotId>,
    pending_key_releases: Vec<DescramblerKeyToken>,
    pid_claims: Vec<DescramblerPidClaim>,
    state: DescramblerSessionState,
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
        self.state == DescramblerSessionState::Closed
    }
    pub(crate) fn is_open(&self) -> bool {
        self.state == DescramblerSessionState::Open
    }
    pub(crate) fn is_quarantined(&self) -> bool {
        self.state == DescramblerSessionState::Quarantined
    }
    pub(crate) fn clear_key(&mut self) -> Option<DescramblerKeyToken> {
        let old = self.key_token.take();
        self.key_slot = None;
        old
    }
    pub(crate) fn set_demux_binding(&mut self, demux_id: i32, generation: u64) {
        self.demux_id = Some(demux_id);
        self.demux_generation = Some(generation);
        self.source_call_state = DescramblerSourceCallState::Bound;
    }
    pub(crate) fn consume_source_call(&mut self) -> bool {
        if self.source_call_state != DescramblerSourceCallState::NeverCalledUnbound {
            return false;
        }
        self.source_call_state = DescramblerSourceCallState::CallConsumedUnbound { failure: None };
        true
    }
    pub(crate) fn source_call_is_consumed_unbound(&self) -> bool {
        matches!(
            self.source_call_state,
            DescramblerSourceCallState::CallConsumedUnbound { failure: None }
        )
    }
    pub(crate) fn record_source_call_failure(
        &mut self,
        failure: DescramblerSourceCallFailure,
    ) -> bool {
        if !matches!(
            self.source_call_state,
            DescramblerSourceCallState::CallConsumedUnbound { .. }
        ) {
            return false;
        }
        self.source_call_state =
            DescramblerSourceCallState::CallConsumedUnbound { failure: Some(failure) };
        true
    }
    pub(crate) fn clear_pid_claims(&mut self) {
        self.pid_claims.clear();
    }
    pub(crate) fn set_key(&mut self, token: DescramblerKeyToken, key_slot: DescramblerKeySlotId) {
        self.key_token = Some(token);
        self.key_slot = Some(key_slot);
    }
    pub(crate) fn add_pending_key_release(&mut self, token: DescramblerKeyToken) {
        if self.key_token.as_ref() != Some(&token) && !self.pending_key_releases.contains(&token) {
            self.pending_key_releases.push(token);
        }
    }
    pub(crate) fn add_pid_claim(&mut self, claim: DescramblerPidClaim) {
        if self.pid_claims.contains(&claim) {
            return;
        }
        self.pid_claims.retain(|stored| stored.pid() != claim.pid());
        self.pid_claims.push(claim);
    }
    pub(crate) fn remove_pid_claim(&mut self, claim: DescramblerPidClaim) {
        self.pid_claims.retain(|item| *item != claim);
    }
    pub(crate) fn close_all(&mut self) {
        self.pid_claims.clear();
        self.clear_key();
        self.pending_key_releases.clear();
        self.demux_id = None;
        self.demux_generation = None;
        self.state = DescramblerSessionState::Closed;
    }
    pub(crate) fn quarantine(&mut self) {
        self.state = DescramblerSessionState::Quarantined;
    }
    pub(crate) fn mark_cleanup_pending(&mut self) {
        self.state = DescramblerSessionState::CleanupPending;
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
    #[cfg(test)]
    pub(crate) fn key_token(&self) -> Option<&DescramblerKeyToken> {
        self.session.key_token()
    }
    pub(crate) fn demux_binding(&self) -> Option<(i32, u64)> {
        Some((self.session.demux_id()?, self.session.demux_generation()?))
    }

    pub(crate) fn is_bound_to_demux(&self, demux_id: i32, generation: u64) -> bool {
        self.session.is_open()
            && self.session.demux_id() == Some(demux_id)
            && self.session.demux_generation() == Some(generation)
    }

    pub(crate) fn holds_binding_to_demux(&self, demux_id: i32, generation: u64) -> bool {
        !self.session.is_closed()
            && self.session.demux_id() == Some(demux_id)
            && self.session.demux_generation() == Some(generation)
    }

    pub(crate) fn holds_binding_to_demux_id(&self, demux_id: i32) -> bool {
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

    #[cfg(test)]
    pub(crate) fn is_quarantined(&self) -> bool {
        self.session.is_quarantined()
    }

    pub(crate) fn begin_demux_source_call_use_case(
        &mut self,
    ) -> Result<(), DescramblerSessionFailure> {
        begin_demux_source_call_use_case(&mut self.session)
    }

    pub(crate) fn commit_demux_binding_use_case(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        commit_demux_binding_use_case(&mut self.session, demux_id, demux_generation)
    }

    pub(crate) fn record_demux_source_call_failure_use_case(
        &mut self,
        failure: DescramblerSourceCallFailure,
    ) -> bool {
        self.session.record_source_call_failure(failure)
    }

    #[cfg(test)]
    pub(crate) fn source_call_failure(&self) -> Option<DescramblerSourceCallFailure> {
        match self.session.source_call_state {
            DescramblerSourceCallState::CallConsumedUnbound { failure } => failure,
            DescramblerSourceCallState::NeverCalledUnbound | DescramblerSourceCallState::Bound => {
                None
            }
        }
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
    ) -> Result<DescramblerClearKeyOutcome<DescramblerKeyLookupError>, DescramblerClearKeyTxnError>
    {
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
    DemuxAlreadyBound,
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
struct DescramblerTxnJournal {
    steps: Vec<DescramblerSessionTxnStep>,
}

impl DescramblerTxnJournal {
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
        if !session.is_open() {
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })
        } else {
            Ok(())
        }
    }

    fn consume_demux_source_call(
        &mut self,
        session: &mut DescramblerSession,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ValidateDemux);
        if !session.consume_source_call() {
            return Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateDemux,
                kind: DescramblerSessionFailureKind::DemuxAlreadyBound,
            });
        }
        self.record_step(DescramblerSessionTxnStep::Commit);
        Ok(())
    }

    fn commit_demux_binding(
        &mut self,
        session: &mut DescramblerSession,
        demux_id: i32,
        generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        self.ensure_open(session)?;
        self.record_step(DescramblerSessionTxnStep::ValidateDemux);
        if !session.source_call_is_consumed_unbound()
            || session.demux_id().is_some()
            || session.demux_generation().is_some()
        {
            return Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateDemux,
                kind: DescramblerSessionFailureKind::DemuxAlreadyBound,
            });
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
            (Some(token), Some(key_slot)) => {
                Ok(DescramblerClearKeyPlan::ClearExisting { token, key_slot })
            }
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

/// Owns only the demux-binding mutation. Key, PID and cleanup mutations have
/// separate transaction owners below.
struct DescramblerBindingTxn<'a> {
    session: &'a mut DescramblerSession,
}

impl<'a> DescramblerBindingTxn<'a> {
    fn new(session: &'a mut DescramblerSession) -> Self {
        Self { session }
    }

    fn consume_source_call(self) -> Result<(), DescramblerSessionFailure> {
        DescramblerTxnJournal::new().consume_demux_source_call(self.session)
    }

    fn commit_binding(
        self,
        demux_id: i32,
        generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        DescramblerTxnJournal::new().commit_demux_binding(self.session, demux_id, generation)
    }
}

/// Canonical owner for normal descrambler PID relation mutations.
pub(crate) struct DescramblerPidTxn<'a> {
    session: &'a mut DescramblerSession,
}

impl<'a> DescramblerPidTxn<'a> {
    fn new(session: &'a mut DescramblerSession) -> Self {
        Self { session }
    }

    fn add(self, claim: DescramblerPidClaim) -> Result<(), DescramblerSessionFailure> {
        DescramblerTxnJournal::new().add_pid_claim(self.session, claim)
    }

    fn remove(self, claim: DescramblerPidClaim) -> Result<(), DescramblerSessionFailure> {
        DescramblerTxnJournal::new().remove_pid_claim(self.session, claim)
    }
}

pub(crate) fn bind_demux_use_case(
    session: &mut DescramblerSession,
    demux_id: i32,
    generation: u64,
) -> Result<(), DescramblerSessionFailure> {
    begin_demux_source_call_use_case(session)?;
    commit_demux_binding_use_case(session, demux_id, generation)
}

pub(crate) fn begin_demux_source_call_use_case(
    session: &mut DescramblerSession,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerBindingTxn::new(session).consume_source_call()
}

pub(crate) fn commit_demux_binding_use_case(
    session: &mut DescramblerSession,
    demux_id: i32,
    generation: u64,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerBindingTxn::new(session).commit_binding(demux_id, generation)
}

fn plan_replace_key_use_case(
    session: &DescramblerSession,
    token: &DescramblerKeyToken,
) -> Result<DescramblerReplaceKeyPlan, DescramblerSessionFailure> {
    DescramblerTxnJournal::new().plan_replace_key(session, token)
}

fn commit_replace_key_use_case(
    session: &mut DescramblerSession,
    plan: DescramblerReplaceKeyPlan,
    token: DescramblerKeyToken,
    key_slot: DescramblerKeySlotId,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerTxnJournal::new().commit_validated_replace_key(session, plan, token, key_slot)
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
    ClearedWithOldKeyReleaseFailure { release_old: ReleaseError },
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
    ReplacedWithOldKeyReleaseFailure { release_old: ReleaseError },
}

/// Canonical owner for key acquire/apply/session commit/old-reference release.
pub(crate) struct DescramblerKeyTxn<'a, KeyTable> {
    session: &'a mut DescramblerSession,
    key_table: &'a mut KeyTable,
}

impl<'a, KeyTable> DescramblerKeyTxn<'a, KeyTable>
where
    KeyTable: DescramblerKeyTxnOps,
{
    fn new(session: &'a mut DescramblerSession, key_table: &'a mut KeyTable) -> Self {
        Self { session, key_table }
    }

    fn clear(
        &mut self,
    ) -> Result<DescramblerClearKeyOutcome<KeyTable::LookupError>, DescramblerClearKeyTxnError>
    {
        let prepared = prepare_clear_key_use_case(self.session)
            .map_err(DescramblerClearKeyTxnError::Session)?;
        let old_token = prepared.plan.old_token().cloned();
        commit_prepared_clear_key_use_case(self.session, prepared)
            .map_err(DescramblerClearKeyTxnError::Session)?;
        let Some(token) = old_token.as_ref() else {
            return Ok(DescramblerClearKeyOutcome::AlreadyClear);
        };
        if let Err(release_old) = self.key_table.release_key_token(token) {
            self.session.add_pending_key_release(token.clone());
            self.session.quarantine();
            return Ok(DescramblerClearKeyOutcome::ClearedWithOldKeyReleaseFailure {
                release_old,
            });
        }
        Ok(DescramblerClearKeyOutcome::Cleared)
    }

    fn replace(
        &mut self,
        token: DescramblerKeyToken,
    ) -> Result<
        DescramblerReplaceKeyOutcome<KeyTable::LookupError>,
        DescramblerReplaceKeyTxnError<KeyTable::LookupError, KeyTable::LookupError>,
    > {
        let plan = plan_replace_key_use_case(self.session, &token)
            .map_err(DescramblerReplaceKeyTxnError::Session)?;
        if !plan.requires_replace() {
            return Ok(DescramblerReplaceKeyOutcome::AlreadyCurrent);
        }
        let old_token = match &plan.inner {
            DescramblerReplaceKeyPlanKind::AlreadyCurrent { .. } => None,
            DescramblerReplaceKeyPlanKind::Replace { old_token } => old_token.clone(),
        };
        let key_slot = self
            .key_table
            .acquire_key_slot(&token)
            .map_err(DescramblerReplaceKeyTxnError::Acquire)?;
        if let Err(failure) =
            commit_replace_key_use_case(self.session, plan, token.clone(), key_slot)
        {
            let rollback_release = self.key_table.release_key_token(&token).err();
            if rollback_release.is_some() {
                self.session.add_pending_key_release(token);
                self.session.quarantine();
            }
            return Err(DescramblerReplaceKeyTxnError::Commit {
                failure,
                rollback_release,
            });
        }
        if let Some(old_token) = old_token.as_ref() {
            if let Err(release_old) = self.key_table.release_key_token(old_token) {
                self.session.add_pending_key_release(old_token.clone());
                self.session.quarantine();
                return Ok(
                    DescramblerReplaceKeyOutcome::ReplacedWithOldKeyReleaseFailure {
                        release_old,
                    },
                );
            }
        }
        Ok(DescramblerReplaceKeyOutcome::Replaced)
    }
}

/// Canonical owner for terminal session cleanup. Normal PID and key mutation
/// entry points never construct this owner.
pub(crate) struct DescramblerSessionCleanupTxn<'a, KeyTable> {
    session: &'a mut DescramblerSession,
    key_table: &'a mut KeyTable,
}

impl<'a, KeyTable> DescramblerSessionCleanupTxn<'a, KeyTable>
where
    KeyTable: DescramblerKeyTxnOps,
{
    fn new(session: &'a mut DescramblerSession, key_table: &'a mut KeyTable) -> Self {
        Self { session, key_table }
    }

    fn cleanup(
        &mut self,
    ) -> Result<DescramblerCleanupReport, DescramblerCleanupTxnError<KeyTable::LookupError>> {
        let mut release_error = None;
        if let Some(current_token) = self.session.key_token().cloned() {
            match self.key_table.release_key_token(&current_token) {
                Ok(()) => {
                    self.session.clear_key();
                }
                Err(error) => {
                    release_error = Some(error);
                }
            }
        }
        let pending = core::mem::take(&mut self.session.pending_key_releases);
        for token in pending {
            if let Err(error) = self.key_table.release_key_token(&token) {
                self.session.add_pending_key_release(token);
                if release_error.is_none() {
                    release_error = Some(error);
                }
            }
        }
        self.session.clear_pid_claims();
        if let Some(error) = release_error {
            self.session.mark_cleanup_pending();
            return Err(DescramblerCleanupTxnError::ReleaseKey(error));
        }
        let report = DescramblerTxnJournal::new().cleanup_all(self.session);
        match report.failure() {
            None => Ok(report),
            Some(failure) => Err(DescramblerCleanupTxnError::Session(failure)),
        }
    }
}

pub(crate) fn clear_key_use_case<KeyTable>(
    session: &mut DescramblerSession,
    key_table: &mut KeyTable,
) -> Result<DescramblerClearKeyOutcome<KeyTable::LookupError>, DescramblerClearKeyTxnError>
where
    KeyTable: DescramblerKeyTxnOps,
{
    DescramblerKeyTxn::new(session, key_table).clear()
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
    DescramblerKeyTxn::new(session, key_table).replace(token)
}

pub(crate) fn add_pid_claim_use_case(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerPidTxn::new(session).add(claim)
}

pub(crate) fn remove_pid_claim_use_case(
    session: &mut DescramblerSession,
    claim: DescramblerPidClaim,
) -> Result<(), DescramblerSessionFailure> {
    DescramblerPidTxn::new(session).remove(claim)
}

fn prepare_clear_key_use_case(
    session: &DescramblerSession,
) -> Result<PreparedDescramblerClearKey, DescramblerSessionFailure> {
    let mut txn = DescramblerTxnJournal::new();
    let plan = txn.plan_clear_key(session)?;
    txn.validate_clear_key_plan(session, &plan)?;
    Ok(PreparedDescramblerClearKey::new(plan))
}

fn commit_prepared_clear_key_use_case(
    session: &mut DescramblerSession,
    prepared: PreparedDescramblerClearKey,
) -> Result<(), DescramblerSessionFailure> {
    let mut txn = DescramblerTxnJournal::new();
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
    DescramblerSessionCleanupTxn::new(session, key_table).cleanup()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FaultKeyTable {
        fail_release: bool,
        released: Vec<DescramblerKeyToken>,
    }

    impl DescramblerKeyTxnOps for FaultKeyTable {
        type LookupError = &'static str;

        fn acquire_key_slot(
            &mut self,
            _token: &DescramblerKeyToken,
        ) -> Result<DescramblerKeySlotId, Self::LookupError> {
            Ok(DescramblerKeySlotId(7))
        }

        fn release_key_token(
            &mut self,
            token: &DescramblerKeyToken,
        ) -> Result<(), Self::LookupError> {
            if self.fail_release {
                return Err("injected release failure");
            }
            self.released.push(token.clone());
            Ok(())
        }
    }

    #[test]
    fn pid_txn_is_idempotent_for_same_source_and_replaces_different_source() {
        let mut session = DescramblerSession::new();
        bind_demux_use_case(&mut session, 3, 9).unwrap();
        let first = DescramblerPidClaim::from_source_filter(0x0100, 20, 4).unwrap();
        let replacement = DescramblerPidClaim::from_source_filter(0x0100, 21, 4).unwrap();

        add_pid_claim_use_case(&mut session, first).unwrap();
        add_pid_claim_use_case(&mut session, first).unwrap();
        assert_eq!(session.pid_claims(), &[first]);

        add_pid_claim_use_case(&mut session, replacement).unwrap();
        assert_eq!(session.pid_claims(), &[replacement]);
    }

    #[test]
    fn demux_source_binding_is_one_shot() {
        let mut session = DescramblerSession::new();
        bind_demux_use_case(&mut session, 3, 9).unwrap();

        let same = bind_demux_use_case(&mut session, 3, 9).unwrap_err();
        let different = bind_demux_use_case(&mut session, 4, 10).unwrap_err();

        assert_eq!(same.kind, DescramblerSessionFailureKind::DemuxAlreadyBound);
        assert_eq!(
            different.kind,
            DescramblerSessionFailureKind::DemuxAlreadyBound
        );
        assert_eq!(session.demux_id(), Some(3));
        assert_eq!(session.demux_generation(), Some(9));
    }

    #[test]
    fn key_release_failure_quarantines_and_cleanup_retries_the_exact_token() {
        let mut session = DescramblerSession::new();
        bind_demux_use_case(&mut session, 3, 9).unwrap();
        let token = DescramblerKeyToken::try_from_bytes(vec![0x55; 8]).unwrap();
        let mut keys = FaultKeyTable::default();
        assert_eq!(
            replace_key_use_case(&mut session, &mut keys, token.clone()).unwrap(),
            DescramblerReplaceKeyOutcome::Replaced
        );
        keys.fail_release = true;

        assert_eq!(
            clear_key_use_case(&mut session, &mut keys).unwrap(),
            DescramblerClearKeyOutcome::ClearedWithOldKeyReleaseFailure {
                release_old: "injected release failure",
            }
        );
        assert!(session.is_quarantined());
        assert_eq!(session.pending_key_releases, vec![token.clone()]);

        keys.fail_release = false;
        cleanup_all_use_case(&mut session, &mut keys).unwrap();
        assert!(session.is_closed());
        assert!(session.pending_key_releases.is_empty());
        assert_eq!(keys.released, vec![token]);
    }

    #[test]
    fn cleanup_release_failure_keeps_capacity_pending_until_retry_succeeds() {
        let mut session = DescramblerSession::new();
        bind_demux_use_case(&mut session, 4, 10).unwrap();
        let token = DescramblerKeyToken::try_from_bytes(vec![0x56; 8]).unwrap();
        let claim = DescramblerPidClaim::from_demux_input(0x0100).unwrap();
        add_pid_claim_use_case(&mut session, claim).unwrap();
        let mut keys = FaultKeyTable::default();
        replace_key_use_case(&mut session, &mut keys, token.clone()).unwrap();
        keys.fail_release = true;

        assert!(matches!(
            cleanup_all_use_case(&mut session, &mut keys),
            Err(DescramblerCleanupTxnError::ReleaseKey(
                "injected release failure"
            ))
        ));
        assert_eq!(session.state, DescramblerSessionState::CleanupPending);
        assert_eq!(session.key_token(), Some(&token));
        assert!(session.pid_claims().is_empty());
        assert_eq!(session.demux_id(), Some(4));

        keys.fail_release = false;
        cleanup_all_use_case(&mut session, &mut keys).unwrap();
        assert!(session.is_closed());
        assert_eq!(keys.released, vec![token]);
    }
}
