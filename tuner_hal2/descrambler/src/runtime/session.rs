use super::{DescramblerKeySlotId, DescramblerKeyToken, DescramblerPidClaim};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DescramblerSession {
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

    pub fn demux_id(&self) -> Option<i32> {
        self.demux_id
    }
    pub fn demux_generation(&self) -> Option<u64> {
        self.demux_generation
    }
    pub fn key_slot(&self) -> Option<DescramblerKeySlotId> {
        self.key_slot
    }
    pub fn key_token(&self) -> Option<&DescramblerKeyToken> {
        self.key_token.as_ref()
    }
    pub fn pid_claims(&self) -> &[DescramblerPidClaim] {
        &self.pid_claims
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub(crate) fn clear_key(&mut self) -> Option<DescramblerKeyToken> {
        let old = self.key_token.take();
        self.key_slot = None;
        old
    }
    pub fn replace_key(
        &mut self,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlotId,
    ) -> Option<DescramblerKeyToken> {
        let old = self.key_token.replace(token);
        self.key_slot = Some(key_slot);
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

#[derive(Debug)]
pub struct DescramblerRuntime {
    descrambler_id: i32,
    session: DescramblerSession,
}

impl DescramblerRuntime {
    pub fn new(descrambler_id: i32) -> Self {
        Self {
            descrambler_id,
            session: DescramblerSession::new(),
        }
    }
    pub fn descrambler_id(&self) -> i32 {
        self.descrambler_id
    }
    pub fn session(&self) -> &DescramblerSession {
        &self.session
    }
    pub fn session_mut(&mut self) -> &mut DescramblerSession {
        &mut self.session
    }
}
