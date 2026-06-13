use super::{DescramblerKeySlotId, DescramblerPidClaim};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DescramblerSession {
    demux_id: Option<i32>,
    key_slot: Option<DescramblerKeySlotId>,
    pid_claims: Vec<DescramblerPidClaim>,
    closed: bool,
}

impl DescramblerSession {
    pub fn new() -> Self { Self::default() }

    pub fn demux_id(&self) -> Option<i32> { self.demux_id }
    pub fn key_slot(&self) -> Option<DescramblerKeySlotId> { self.key_slot }
    pub fn pid_claims(&self) -> &[DescramblerPidClaim] { &self.pid_claims }
    pub fn is_closed(&self) -> bool { self.closed }

    pub(crate) fn snapshot(&self) -> DescramblerSessionSnapshot { DescramblerSessionSnapshot(self.clone()) }
    pub(crate) fn restore(&mut self, snapshot: DescramblerSessionSnapshot) { *self = snapshot.0; }
    pub(crate) fn set_demux_id(&mut self, demux_id: i32) { self.demux_id = Some(demux_id); }
    pub(crate) fn set_key_slot(&mut self, key_slot: DescramblerKeySlotId) { self.key_slot = Some(key_slot); }
    pub(crate) fn add_pid_claim(&mut self, claim: DescramblerPidClaim) {
        if !self.pid_claims.contains(&claim) {
            self.pid_claims.push(claim);
        }
    }
    pub(crate) fn remove_pid_claim(&mut self, claim: DescramblerPidClaim) { self.pid_claims.retain(|item| *item != claim); }
    pub(crate) fn close_all(&mut self) {
        self.pid_claims.clear();
        self.key_slot = None;
        self.demux_id = None;
        self.closed = true;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescramblerSessionSnapshot(DescramblerSession);

#[derive(Debug)]
pub struct DescramblerRuntime {
    descrambler_id: i32,
    session: DescramblerSession,
}

impl DescramblerRuntime {
    pub fn new(descrambler_id: i32) -> Self { Self { descrambler_id, session: DescramblerSession::new() } }
    pub fn descrambler_id(&self) -> i32 { self.descrambler_id }
    pub fn session(&self) -> &DescramblerSession { &self.session }
    pub fn session_mut(&mut self) -> &mut DescramblerSession { &mut self.session }
}
