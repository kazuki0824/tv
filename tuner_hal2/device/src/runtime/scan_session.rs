//! frontend scan session所有。
//!
//! scan sessionは単なるbackend tune loopではなく、candidate順序、現在candidate index、終端phase、終端reasonを所有する。
//! scan行を完了扱いにするには、callback配送とlive pump統合がこの状態を消費しなければならない。

use maleicacid_tuner_hal2_common::{FrontendTuneRequest, HalError, HalInternalKind};

use super::FrontendWorkerCancelReason;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendScanPhase {
    Running,
    LockedReported,
    Completed,
    Cancelled,
    FailedBackend,
    FailedCallback,
    FailedPanic,
}

impl FrontendScanPhase {
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    pub const fn is_failed(self) -> bool {
        matches!(
            self,
            Self::FailedBackend | Self::FailedCallback | Self::FailedPanic
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendScanTerminalReason {
    End,
    StopRequested,
    SupersededByNewRequest,
    FrontendClosing,
    BackendFailure,
    CallbackFailure,
    PanicOrJoinFailure,
}

impl From<FrontendWorkerCancelReason> for FrontendScanTerminalReason {
    fn from(reason: FrontendWorkerCancelReason) -> Self {
        match reason {
            FrontendWorkerCancelReason::StopRequested => Self::StopRequested,
            FrontendWorkerCancelReason::SupersededByNewRequest => Self::SupersededByNewRequest,
            FrontendWorkerCancelReason::FrontendClosing => Self::FrontendClosing,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendScanSession {
    generation: u64,
    fingerprint: String,
    candidates: Vec<FrontendTuneRequest>,
    current_index: usize,
    phase: FrontendScanPhase,
    terminal_reason: Option<FrontendScanTerminalReason>,
}

impl FrontendScanSession {
    pub fn start(
        generation: u64,
        fingerprint: impl Into<String>,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<Self, HalError> {
        if generation == 0 {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan session generation must be non-zero",
            ));
        }
        if candidates.is_empty() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan session requires at least one explicit candidate",
            ));
        }
        Ok(Self {
            generation,
            fingerprint: fingerprint.into(),
            candidates,
            current_index: 0,
            phase: FrontendScanPhase::Running,
            terminal_reason: None,
        })
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub const fn phase(&self) -> FrontendScanPhase {
        self.phase
    }
    pub const fn terminal_reason(&self) -> Option<FrontendScanTerminalReason> {
        self.terminal_reason
    }
    pub const fn current_index(&self) -> usize {
        self.current_index
    }
    pub fn candidates(&self) -> &[FrontendTuneRequest] {
        &self.candidates
    }

    pub fn current_candidate(&self) -> Option<&FrontendTuneRequest> {
        if self.phase.is_terminal() {
            return None;
        }
        self.candidates.get(self.current_index)
    }

    pub fn advance_after_candidate(&mut self) -> Result<Option<&FrontendTuneRequest>, HalError> {
        self.ensure_running()?;
        let next = self.current_index.checked_add(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan candidate index overflow",
            )
        })?;
        if next >= self.candidates.len() {
            self.complete();
            return Ok(None);
        }
        self.current_index = next;
        Ok(self.candidates.get(self.current_index))
    }

    pub fn cancel(&mut self, reason: FrontendWorkerCancelReason) {
        if self.phase.is_failed() {
            return;
        }
        self.phase = FrontendScanPhase::Cancelled;
        self.terminal_reason = Some(reason.into());
    }

    pub fn mark_locked_reported(&mut self) -> Result<(), HalError> {
        self.ensure_running()?;
        self.phase = FrontendScanPhase::LockedReported;
        self.terminal_reason = None;
        Ok(())
    }

    pub fn fail_backend(&mut self) {
        self.phase = FrontendScanPhase::FailedBackend;
        self.terminal_reason = Some(FrontendScanTerminalReason::BackendFailure);
    }

    pub fn fail_callback(&mut self) {
        self.phase = FrontendScanPhase::FailedCallback;
        self.terminal_reason = Some(FrontendScanTerminalReason::CallbackFailure);
    }

    pub fn fail_panic_or_join(&mut self) {
        self.phase = FrontendScanPhase::FailedPanic;
        self.terminal_reason = Some(FrontendScanTerminalReason::PanicOrJoinFailure);
    }

    pub fn complete(&mut self) {
        if self.phase.is_failed() {
            return;
        }
        self.phase = FrontendScanPhase::Completed;
        self.terminal_reason = Some(FrontendScanTerminalReason::End);
    }

    fn ensure_running(&self) -> Result<(), HalError> {
        if self.phase != FrontendScanPhase::Running {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan session is not running",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::FrontendSystem;

    fn request(frequency: u64) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        }
    }
    #[test]
    fn scan_session_rejects_empty_candidates() {
        assert!(FrontendScanSession::start(1, "empty", Vec::new()).is_err());
    }

    #[test]
    fn candidate_progression_completes_after_last_candidate() {
        let mut session =
            FrontendScanSession::start(4, "two", vec![request(473_142_857), request(479_142_857)])
                .unwrap();
        assert_eq!(session.current_candidate().unwrap().frequency, 473_142_857);
        assert_eq!(
            session
                .advance_after_candidate()
                .unwrap()
                .unwrap()
                .frequency,
            479_142_857
        );
        assert!(session.advance_after_candidate().unwrap().is_none());
        assert_eq!(session.phase(), FrontendScanPhase::Completed);
        assert_eq!(
            session.terminal_reason(),
            Some(FrontendScanTerminalReason::End)
        );
    }

    #[test]
    fn cancellation_preserves_reason() {
        let mut session =
            FrontendScanSession::start(5, "cancel", vec![request(473_142_857)]).unwrap();
        session.cancel(FrontendWorkerCancelReason::SupersededByNewRequest);
        assert_eq!(session.phase(), FrontendScanPhase::Cancelled);
        assert_eq!(
            session.terminal_reason(),
            Some(FrontendScanTerminalReason::SupersededByNewRequest)
        );
        assert!(session.current_candidate().is_none());
    }

    #[test]
    fn failed_phase_is_not_overwritten_by_cancel_or_complete() {
        let mut session =
            FrontendScanSession::start(6, "fail", vec![request(473_142_857)]).unwrap();
        session.fail_backend();
        session.cancel(FrontendWorkerCancelReason::StopRequested);
        session.complete();
        assert_eq!(session.phase(), FrontendScanPhase::FailedBackend);
        assert_eq!(
            session.terminal_reason(),
            Some(FrontendScanTerminalReason::BackendFailure)
        );
    }

    #[test]
    fn locked_reported_waits_for_a_new_scan_continuation() {
        let mut session =
            FrontendScanSession::start(7, "locked", vec![request(473_142_857)]).unwrap();
        session.mark_locked_reported().unwrap();
        assert_eq!(session.phase(), FrontendScanPhase::LockedReported);
        assert_eq!(session.terminal_reason(), None);
        assert!(session.current_candidate().is_none());
    }
}
