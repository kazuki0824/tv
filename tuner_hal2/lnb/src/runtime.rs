use crate::{LnbFailureKind, LnbFailureRecord, LnbFailureStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbRuntimeState {
    Open,
    Applying,
    Closing,
    Failed,
    Closed,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbVoltage {
    Off,
    V13,
    V18,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbTone {
    Off,
    On,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LnbElectricalState {
    pub voltage: LnbVoltage,
    pub tone: LnbTone,
    pub satellite_position: Option<i32>,
}

impl LnbElectricalState {
    pub const fn safe() -> Self {
        Self { voltage: LnbVoltage::Off, tone: LnbTone::Off, satellite_position: None }
    }
}

#[derive(Debug)]
pub struct LnbRuntime {
    lnb_id: i32,
    state: LnbRuntimeState,
    registry_state: LnbElectricalState,
    backend_committed_state: LnbElectricalState,
    callback_registered: bool,
    last_failure: Option<LnbFailureRecord>,
    force_next_registry_commit_failure: Option<LnbFailureKind>,
    drop_leak_recorded: bool,
}

impl LnbRuntime {
    pub fn new(lnb_id: i32) -> Self {
        let safe = LnbElectricalState::safe();
        Self {
            lnb_id,
            state: LnbRuntimeState::Open,
            registry_state: safe,
            backend_committed_state: safe,
            callback_registered: false,
            last_failure: None,
            force_next_registry_commit_failure: None,
            drop_leak_recorded: false,
        }
    }

    pub fn lnb_id(&self) -> i32 { self.lnb_id }
    pub fn state(&self) -> LnbRuntimeState { self.state }
    pub fn registry_state(&self) -> LnbElectricalState { self.registry_state }
    pub fn backend_committed_state(&self) -> LnbElectricalState { self.backend_committed_state }
    pub fn callback_registered(&self) -> bool { self.callback_registered }
    pub fn last_failure(&self) -> Option<&LnbFailureRecord> { self.last_failure.as_ref() }
    pub fn drop_leak_recorded(&self) -> bool { self.drop_leak_recorded }

    pub fn set_callback_registered(&mut self, registered: bool) {
        if matches!(self.state, LnbRuntimeState::Open | LnbRuntimeState::Applying) {
            self.callback_registered = registered;
        }
    }

    pub fn inject_next_registry_commit_failure(&mut self, kind: LnbFailureKind) {
        self.force_next_registry_commit_failure = Some(kind);
    }

    pub(crate) fn begin_apply(&mut self) -> Result<(), LnbFailureRecord> {
        match self.state {
            LnbRuntimeState::Open => {
                self.state = LnbRuntimeState::Applying;
                Ok(())
            }
            _ => Err(self.record_failure(LnbFailureKind::InvalidState, LnbFailureStep::ValidateState)),
        }
    }

    pub(crate) fn begin_close(&mut self) -> Result<(), LnbFailureRecord> {
        match self.state {
            LnbRuntimeState::Open | LnbRuntimeState::Failed => {
                self.state = LnbRuntimeState::Closing;
                Ok(())
            }
            LnbRuntimeState::Closed => Ok(()),
            _ => Err(self.record_failure(LnbFailureKind::InvalidState, LnbFailureStep::MarkClosing)),
        }
    }

    pub(crate) fn note_backend_applied(&mut self, state: LnbElectricalState) {
        self.backend_committed_state = state;
    }

    pub(crate) fn commit_registry(&mut self, state: LnbElectricalState, step: LnbFailureStep) -> Result<(), LnbFailureRecord> {
        if let Some(kind) = self.force_next_registry_commit_failure.take() {
            return Err(self.record_failure(kind, step));
        }
        self.registry_state = state;
        Ok(())
    }

    pub(crate) fn commit_open(&mut self) {
        self.state = LnbRuntimeState::Open;
    }

    pub(crate) fn commit_closed(&mut self) {
        self.state = LnbRuntimeState::Closed;
        self.callback_registered = false;
    }

    pub(crate) fn clear_callback(&mut self) {
        self.callback_registered = false;
    }

    pub(crate) fn record_failure(&mut self, kind: LnbFailureKind, step: LnbFailureStep) -> LnbFailureRecord {
        self.state = LnbRuntimeState::Failed;
        let record = LnbFailureRecord { lnb_id: self.lnb_id, kind, step };
        self.last_failure = Some(record.clone());
        record
    }

    pub(crate) fn quarantine(&mut self, kind: LnbFailureKind, step: LnbFailureStep) -> LnbFailureRecord {
        self.state = LnbRuntimeState::Quarantined;
        let record = LnbFailureRecord { lnb_id: self.lnb_id, kind, step };
        self.last_failure = Some(record.clone());
        record
    }

    pub fn record_unclosed_drop(&mut self) -> LnbFailureRecord {
        self.drop_leak_recorded = true;
        self.quarantine(LnbFailureKind::DropWithoutClose, LnbFailureStep::DropLeakRecord)
    }
}

pub trait LnbBackendOps {
    fn apply_lnb_state(&mut self, lnb_id: i32, state: LnbElectricalState) -> Result<(), LnbFailureKind>;
    fn clear_lnb_callback(&mut self, lnb_id: i32) -> Result<(), LnbFailureKind>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBackend {
        applied: Vec<LnbElectricalState>,
        clear_count: usize,
        fail_apply: Option<LnbFailureKind>,
        fail_clear: Option<LnbFailureKind>,
    }

    impl TestBackend {
        fn new() -> Self { Self { applied: Vec::new(), clear_count: 0, fail_apply: None, fail_clear: None } }
    }

    impl LnbBackendOps for TestBackend {
        fn apply_lnb_state(&mut self, _lnb_id: i32, state: LnbElectricalState) -> Result<(), LnbFailureKind> {
            if let Some(kind) = self.fail_apply.take() { return Err(kind); }
            self.applied.push(state);
            Ok(())
        }

        fn clear_lnb_callback(&mut self, _lnb_id: i32) -> Result<(), LnbFailureKind> {
            if let Some(kind) = self.fail_clear.take() { return Err(kind); }
            self.clear_count += 1;
            Ok(())
        }
    }

    #[test]
    fn drop_leak_records_quarantine_without_backend_cleanup() {
        let mut runtime = LnbRuntime::new(7);
        let record = runtime.record_unclosed_drop();
        assert_eq!(record.kind, LnbFailureKind::DropWithoutClose);
        assert_eq!(runtime.state(), LnbRuntimeState::Quarantined);
        assert!(runtime.drop_leak_recorded());
    }
}
