use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::os_abi::{
    last_errno, raw_ioctl_noarg, raw_ioctl_ptr, raw_ioctl_word,
};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendBackendKind, FrontendDevicePath,
    FrontendIsdbtPartialReceptionRequirement, FrontendSystem, FrontendTuneRequest, HalError,
    HalErrorDetail, HalInternalKind, HalInvalidArgumentKind,
};

use super::reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
use super::thread_result_owner::{ThreadResultOwner, ThreadResultPoll};
use super::tune_txn::{BackendTuneOps, BackendTuneOutcome, BackendTuneStep, BackendTuneTxn};
use crate::dvb;
use crate::dvb::abi::{
    DtvProperties, DtvProperty, DTV_CLEAR, FE_HAS_CARRIER, FE_HAS_LOCK, FE_READ_STATUS,
    FE_SET_PROPERTY, FE_SET_VOLTAGE, SEC_VOLTAGE_13, SEC_VOLTAGE_18, SEC_VOLTAGE_OFF,
};
use crate::px4;
use crate::px4::abi::{
    ptx_enable_lnb_power_scalar, ptx_set_system_mode_scalar, PtxFreq, PtxTmccTsidList,
    ERRNO_EAGAIN, ERRNO_EALREADY, ERRNO_EINVAL, ERRNO_ENOSYS, ERRNO_ENOTTY, PTXT_SET_LNB_VOLTAGE,
    PTX_DISABLE_LNB_POWER, PTX_GET_LOCK_STATUS, PTX_GET_TMCC_PARTIAL_RECEPTION,
    PTX_GET_TMCC_TSID_LIST, PTX_SET_CHANNEL, PTX_START_STREAMING, PTX_STOP_STREAMING,
};
use crate::px4::{classify_tmcc_tsid_read, decode_tmcc_tsid_list, Px4TmccTsidListObservation};
use crate::runtime::{FrontendSignalState, FrontendWorkerContext};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendBackendTunePlan {
    frontend_id: i32,
    generation: u64,
    backend: FrontendBackendKind,
    device_path: FrontendDevicePath,
    request: FrontendTuneRequest,
}

impl FrontendBackendTunePlan {
    pub(super) fn worker_identity(&self) -> (i32, u64) {
        (self.frontend_id, self.generation)
    }

    pub fn new(
        frontend_id: i32,
        generation: u64,
        backend: FrontendBackendKind,
        device_path: FrontendDevicePath,
        request: FrontendTuneRequest,
    ) -> Self {
        Self {
            frontend_id,
            generation,
            backend,
            device_path,
            request,
        }
    }

    pub fn validate_worker_generation(&self, worker_generation: u64) -> Result<(), HalError> {
        if self.generation == worker_generation {
            return Ok(());
        }
        Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            format!(
                "frontend backend tune plan generation mismatch: plan={} worker={}",
                self.generation, worker_generation
            ),
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendBackendSessionKind {
    Px4 { control_path: FrontendDevicePath },
    Dvb { frontend_path: FrontendDevicePath },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendTmccPartialReceptionObservation {
    Pending,
    Available(bool),
}

pub type FrontendTmccTsidListObservation = Px4TmccTsidListObservation;

pub struct FrontendBackendSession {
    kind: FrontendBackendSessionKind,
    file: File,
    initial_signal_state: FrontendSignalState,
    partial_reception: FrontendIsdbtPartialReceptionRequirement,
    px4_channel_apply_result: Option<Px4ChannelApplyResult>,
}

impl core::fmt::Debug for FrontendBackendSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrontendBackendSession")
            .field("kind", &self.kind)
            .field("fd", &self.file.as_raw_fd())
            .field("initial_signal_state", &self.initial_signal_state)
            .field("partial_reception", &self.partial_reception)
            .field("px4_channel_apply_result", &self.px4_channel_apply_result)
            .finish()
    }
}

impl FrontendBackendSession {
    pub fn open_and_submit(plan: &FrontendBackendTunePlan) -> Result<Self, HalError> {
        Self::open_and_submit_with_previous(plan, None)
    }

    pub fn open_and_submit_with_previous(
        plan: &FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    ) -> Result<Self, HalError> {
        Self::open_and_submit_with_previous_report(plan, previous_request)
            .map_err(FrontendBackendSubmitFailure::into_error)
    }

    pub fn open_and_submit_with_previous_report(
        plan: &FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    ) -> Result<Self, FrontendBackendSubmitFailure> {
        let mut executor = FrontendBackendTuneExecutor::open(plan.clone(), previous_request)
            .map_err(|error| FrontendBackendSubmitFailure {
                generation: plan.generation,
                error,
                rollback_succeeded: true,
                step: None,
                rollback_failure: None,
            })?;
        let mut txn = BackendTuneTxn::new(plan.frontend_id, plan.generation, plan.request.clone());
        match txn.apply(&mut executor) {
            BackendTuneOutcome::Committed { .. } => {
                executor
                    .into_session()
                    .map_err(|error| FrontendBackendSubmitFailure {
                        generation: plan.generation,
                        error,
                        rollback_succeeded: true,
                        step: None,
                        rollback_failure: None,
                    })
            }
            BackendTuneOutcome::Failed {
                step,
                error,
                rollback,
            } => Err(FrontendBackendSubmitFailure {
                generation: plan.generation,
                error,
                rollback_succeeded: rollback.succeeded(),
                step: Some(step),
                rollback_failure: rollback.failure().cloned(),
            }),
            BackendTuneOutcome::RollbackFailed {
                step,
                error,
                rollback,
            } => Err(FrontendBackendSubmitFailure {
                generation: plan.generation,
                error,
                rollback_succeeded: false,
                step: Some(step),
                rollback_failure: rollback.failure().cloned(),
            }),
        }
    }

    pub fn initial_signal_state(&self) -> FrontendSignalState {
        self.initial_signal_state
    }

    pub fn partial_reception_requirement(&self) -> FrontendIsdbtPartialReceptionRequirement {
        self.partial_reception
    }

    pub fn px4_channel_apply_result(&self) -> Option<Px4ChannelApplyResult> {
        self.px4_channel_apply_result
    }

    pub fn observe_signal_state(&self) -> Result<FrontendSignalState, HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                px4_signal_state_from_readback(read_px4_boolean(
                    control_path,
                    self.file.as_raw_fd(),
                    PTX_GET_LOCK_STATUS,
                    "PTX_GET_LOCK_STATUS",
                ))
            }
            FrontendBackendSessionKind::Dvb { frontend_path } => {
                let mut status: u32 = 0;
                ioctl_ptr(
                    "dvb",
                    Some(frontend_path.as_path().to_path_buf()),
                    self.file.as_raw_fd(),
                    FE_READ_STATUS,
                    &mut status,
                    "FE_READ_STATUS",
                )?;
                if status & FE_HAS_LOCK != 0 {
                    Ok(FrontendSignalState::Locked)
                } else if status & FE_HAS_CARRIER != 0 {
                    Ok(FrontendSignalState::SignalDetected)
                } else {
                    Ok(FrontendSignalState::NoSignal)
                }
            }
        }
    }

    pub fn observe_tmcc_partial_reception(
        &self,
    ) -> Result<FrontendTmccPartialReceptionObservation, HalError> {
        let FrontendBackendSessionKind::Px4 { control_path } = &self.kind else {
            return Err(HalError::Unsupported(
                "TMCC partial reception readback is available only on px4",
            ));
        };
        classify_tmcc_partial_reception_read(read_px4_boolean(
            control_path,
            self.file.as_raw_fd(),
            PTX_GET_TMCC_PARTIAL_RECEPTION,
            "PTX_GET_TMCC_PARTIAL_RECEPTION",
        ))
    }

    pub fn observe_tmcc_tsid_list(&self) -> Result<FrontendTmccTsidListObservation, HalError> {
        let FrontendBackendSessionKind::Px4 { control_path } = &self.kind else {
            return Err(HalError::Unsupported(
                "TMCC TSID list readback is available only on px4",
            ));
        };
        let mut raw = PtxTmccTsidList::default();
        let read = ioctl_ptr(
            "px4",
            Some(control_path.as_path().to_path_buf()),
            self.file.as_raw_fd(),
            PTX_GET_TMCC_TSID_LIST,
            &mut raw,
            "PTX_GET_TMCC_TSID_LIST",
        )
        .and_then(|()| decode_tmcc_tsid_list(control_path, raw));
        classify_tmcc_tsid_read(read)
    }

    pub fn open_live_reader(
        &self,
        descriptor: &FrontendLiveReaderDescriptor,
    ) -> Result<Box<dyn Read + Send>, HalError> {
        match (&self.kind, &descriptor.kind) {
            (
                FrontendBackendSessionKind::Px4 { control_path },
                FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { .. },
            ) => {
                let file = self.file.try_clone().map_err(|error| HalError::Io {
                    backend: "px4",
                    operation: "live reader fd duplication",
                    path: Some(control_path.as_path().to_path_buf()),
                    errno: error.raw_os_error(),
                    detail: HalErrorDetail::new(error.to_string()),
                })?;
                Ok(Box::new(file))
            }
            (
                FrontendBackendSessionKind::Dvb { .. },
                FrontendLiveReaderDescriptorKind::DvbDvrDevice { dvr_path },
            ) => {
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(dvb::abi::O_NONBLOCK)
                    .open(dvr_path.as_path())
                    .map_err(|error| HalError::Io {
                        backend: "dvb",
                        operation: "live dvr reader open",
                        path: Some(dvr_path.as_path().to_path_buf()),
                        errno: error.raw_os_error(),
                        detail: HalErrorDetail::new(error.to_string()),
                    })?;
                Ok(Box::new(file))
            }
            _ => Err(HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "frontend backend session and live reader descriptor kind mismatch",
            )),
        }
    }

    pub fn close(self) -> Result<(), HalError> {
        match self.kind {
            FrontendBackendSessionKind::Dvb { .. } => Ok(()),
            FrontendBackendSessionKind::Px4 { .. } => self.stop(),
        }
    }

    pub fn stop(&self) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => px4_streaming_ioctl(
                control_path,
                self.file.as_raw_fd(),
                PTX_STOP_STREAMING,
                "PTX_STOP_STREAMING",
            ),
            FrontendBackendSessionKind::Dvb { frontend_path } => {
                let mut prop = DtvProperty::with_data(DTV_CLEAR, 0);
                let mut props = DtvProperties {
                    num: 1,
                    props: &mut prop as *mut DtvProperty,
                };
                ioctl_ptr(
                    "dvb",
                    Some(frontend_path.as_path().to_path_buf()),
                    self.file.as_raw_fd(),
                    FE_SET_PROPERTY,
                    &mut props,
                    "FE_SET_PROPERTY(DTV_CLEAR)",
                )
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendBackendSubmitFailure {
    pub generation: u64,
    pub error: HalError,
    pub rollback_succeeded: bool,
    pub step: Option<BackendTuneStep>,
    pub rollback_failure: Option<super::tune_txn::BackendTuneRollbackFailure>,
}

impl FrontendBackendSubmitFailure {
    pub fn cleanup_result(&self) -> Result<(), HalError> {
        if self.rollback_succeeded {
            Ok(())
        } else {
            Err(self.clone().into_error())
        }
    }

    pub fn indeterminate(generation: u64, error: HalError) -> Self {
        frontend_backend_submit_thread_failure(generation, error)
    }

    pub fn into_error(self) -> HalError {
        let Some(step) = self.step else {
            return self.error;
        };
        let rollback_error = self.rollback_failure.as_ref().map(|failure| {
            HalError::cleanup_failed(
                "frontend backend tune rollback",
                format!("step={:?} error={}", failure.step, failure.error),
            )
        });
        let rollback_detail = if self.rollback_succeeded {
            "rollback succeeded"
        } else {
            "rollback failed"
        };
        let cleanup = rollback_error.unwrap_or_else(|| {
            HalError::cleanup_failed(
                "frontend backend tune transaction",
                format!(
                    "generation={} step={step:?} {rollback_detail}",
                    self.generation
                ),
            )
        });
        compose_primary_cleanup_failure("frontend backend submit failure", self.error, cleanup)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendBackendSubmitDisposition {
    Claim,
    Abort,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FrontendBackendSubmitReady {
    Submitted,
    Failed,
}

#[derive(Debug)]
enum FrontendBackendSubmitThreadOutcome {
    Claimed(FrontendBackendSession),
    Failed(FrontendBackendSubmitFailure),
    Aborted(Result<(), HalError>),
}

#[derive(Debug)]
#[must_use = "frontend backend submit ticket must be claimed, aborted, or transferred to the reaper"]
pub(super) struct FrontendBackendSubmitTicket {
    generation: u64,
    ready: Receiver<FrontendBackendSubmitReady>,
    disposition: SyncSender<FrontendBackendSubmitDisposition>,
    owner: Option<ThreadResultOwner<FrontendBackendSubmitThreadOutcome>>,
}

#[derive(Debug)]
pub(super) enum FrontendBackendSubmitWait {
    Completed(Result<FrontendBackendSession, FrontendBackendSubmitFailure>),
    TimedOut(FrontendBackendSubmitTicket),
}

impl FrontendBackendSubmitTicket {
    pub fn start(
        plan: FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    ) -> Result<Self, HalError> {
        let generation = plan.generation;
        Self::start_with(generation, move || {
            FrontendBackendSession::open_and_submit_with_previous_report(&plan, previous_request)
        })
    }

    fn start_with(
        generation: u64,
        submit: impl FnOnce() -> Result<FrontendBackendSession, FrontendBackendSubmitFailure>
            + Send
            + 'static,
    ) -> Result<Self, HalError> {
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let (disposition, disposition_receiver) = mpsc::sync_channel(1);
        let owner = ThreadResultOwner::start("maleicacid-frontend-submit", move || {
            match submit() {
                Ok(session) => {
                    if ready_sender
                        .send(FrontendBackendSubmitReady::Submitted)
                        .is_err()
                    {
                        return Ok(FrontendBackendSubmitThreadOutcome::Aborted(session.close()));
                    }
                    match disposition_receiver.recv() {
                        Ok(FrontendBackendSubmitDisposition::Claim) => {
                            Ok(FrontendBackendSubmitThreadOutcome::Claimed(session))
                        }
                        Ok(FrontendBackendSubmitDisposition::Abort) | Err(_) => {
                            Ok(FrontendBackendSubmitThreadOutcome::Aborted(session.close()))
                        }
                    }
                }
                Err(failure) => {
                    match ready_sender.send(FrontendBackendSubmitReady::Failed) {
                        Ok(()) => {}
                        Err(_) => {
                            // 受信owner消滅後もbackend transactionの型付き結果をthread ownerへ残す。
                        }
                    }
                    Ok(FrontendBackendSubmitThreadOutcome::Failed(failure))
                }
            }
        })?;
        Ok(Self {
            generation,
            ready,
            disposition,
            owner: Some(owner),
        })
    }

    pub fn wait_until(mut self, deadline: Instant) -> Result<FrontendBackendSubmitWait, HalError> {
        let wait = deadline.saturating_duration_since(Instant::now());
        match self.ready.recv_timeout(wait) {
            Ok(FrontendBackendSubmitReady::Submitted) => {
                if self
                    .disposition
                    .send(FrontendBackendSubmitDisposition::Claim)
                    .is_err()
                {
                    // thread終端結果を回収して、backend結果不明として分類する。
                }
                let finished = match self.wait_until_cleanup(Some(deadline)) {
                    Ok(finished) => finished,
                    Err(_) => return Ok(FrontendBackendSubmitWait::TimedOut(self)),
                };
                if !finished {
                    return Ok(FrontendBackendSubmitWait::TimedOut(self));
                }
                let outcome = match self.join_outcome() {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        return Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                };
                match outcome {
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => {
                        Ok(FrontendBackendSubmitWait::Completed(Ok(session)))
                    }
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => {
                        Ok(FrontendBackendSubmitWait::Completed(Err(failure)))
                    }
                    FrontendBackendSubmitThreadOutcome::Aborted(stop_result) => {
                        let error = stop_result.err().unwrap_or_else(|| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend backend submit was aborted after claim",
                            )
                        });
                        Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                }
            }
            Ok(FrontendBackendSubmitReady::Failed) => {
                let finished = match self.wait_until_cleanup(Some(deadline)) {
                    Ok(finished) => finished,
                    Err(_) => return Ok(FrontendBackendSubmitWait::TimedOut(self)),
                };
                if !finished {
                    return Ok(FrontendBackendSubmitWait::TimedOut(self));
                }
                let outcome = match self.join_outcome() {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        return Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                };
                match outcome {
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => {
                        Ok(FrontendBackendSubmitWait::Completed(Err(failure)))
                    }
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => {
                        let stop_result = session.close();
                        let error = stop_result.err().unwrap_or_else(|| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend backend submit reported failure but returned a session",
                            )
                        });
                        Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                    FrontendBackendSubmitThreadOutcome::Aborted(stop_result) => {
                        let error = stop_result.err().unwrap_or_else(|| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend backend submit failure changed to abort",
                            )
                        });
                        Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                match self
                    .disposition
                    .try_send(FrontendBackendSubmitDisposition::Abort)
                {
                    Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
                }
                Ok(FrontendBackendSubmitWait::TimedOut(self))
            }
            Err(RecvTimeoutError::Disconnected) => {
                let finished = match self.wait_until_cleanup(Some(deadline)) {
                    Ok(finished) => finished,
                    Err(_) => return Ok(FrontendBackendSubmitWait::TimedOut(self)),
                };
                if !finished {
                    return Ok(FrontendBackendSubmitWait::TimedOut(self));
                }
                let outcome = match self.join_outcome() {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        return Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                };
                match outcome {
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => {
                        Ok(FrontendBackendSubmitWait::Completed(Err(failure)))
                    }
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => {
                        let stop_error = session.close().err().unwrap_or_else(|| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend backend submit readiness disconnected after success",
                            )
                        });
                        Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, stop_error),
                        )))
                    }
                    FrontendBackendSubmitThreadOutcome::Aborted(stop_result) => {
                        let error = stop_result.err().unwrap_or_else(|| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "frontend backend submit readiness disconnected",
                            )
                        });
                        Ok(FrontendBackendSubmitWait::Completed(Err(
                            frontend_backend_submit_thread_failure(self.generation, error),
                        )))
                    }
                }
            }
        }
    }

    pub(crate) fn wait_until_cleanup(&self, deadline: Option<Instant>) -> Result<bool, HalError> {
        self.owner
            .as_ref()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend backend submit cleanup owner is missing",
                )
            })?
            .wait_until_finished(deadline)
    }

    pub(crate) fn try_complete_cleanup(
        &mut self,
    ) -> Option<Result<(), FrontendBackendSubmitFailure>> {
        let outcome = match self.owner.as_mut()?.collect_if_finished() {
            ThreadResultPoll::Running => return None,
            ThreadResultPoll::Completed(outcome) => outcome,
        };
        self.owner = None;
        Some(frontend_backend_submit_cleanup_result(
            self.generation,
            outcome,
        ))
    }

    pub(crate) fn complete_cleanup(&mut self) -> Result<(), FrontendBackendSubmitFailure> {
        let outcome = self.join_outcome();
        frontend_backend_submit_cleanup_result(self.generation, outcome)
    }

    fn join_outcome(&mut self) -> Result<FrontendBackendSubmitThreadOutcome, HalError> {
        let owner = self.owner.take().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend backend submit owner is missing",
            )
        })?;
        owner.join_after_stop()
    }
}

impl Drop for FrontendBackendSubmitTicket {
    fn drop(&mut self) {
        match self
            .disposition
            .try_send(FrontendBackendSubmitDisposition::Abort)
        {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
        // 終了待ちや機器I/Oはしない。外側のWorkerRuntimeCleanupは結果不明の義務を
        // 隔離して保持するため、ここで結果回収権限が失われても資源を再利用しない。
    }
}

fn frontend_backend_submit_thread_failure(
    generation: u64,
    error: HalError,
) -> FrontendBackendSubmitFailure {
    FrontendBackendSubmitFailure {
        generation,
        error,
        rollback_succeeded: false,
        step: None,
        rollback_failure: None,
    }
}

fn frontend_backend_submit_cleanup_result(
    generation: u64,
    outcome: Result<FrontendBackendSubmitThreadOutcome, HalError>,
) -> Result<(), FrontendBackendSubmitFailure> {
    let result = match outcome {
        Ok(FrontendBackendSubmitThreadOutcome::Aborted(stop_result)) => stop_result,
        Ok(FrontendBackendSubmitThreadOutcome::Failed(failure)) => {
            return Err(failure);
        }
        Ok(FrontendBackendSubmitThreadOutcome::Claimed(session)) => {
            let stop_result = session.close();
            let invariant = HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend backend submit cleanup observed a claimed session",
            );
            match stop_result {
                Ok(()) => Err(invariant),
                Err(stop_error) => Err(compose_primary_cleanup_failure(
                    "frontend backend submit cleanup",
                    invariant,
                    stop_error,
                )),
            }
        }
        Err(error) => Err(error),
    };
    result.map_err(|error| frontend_backend_submit_thread_failure(generation, error))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLnbVoltage {
    None,
    Voltage11V,
    Voltage15V,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendBackendLnbApplyPlan {
    frontend_id: i32,
    backend: FrontendBackendKind,
    device_path: FrontendDevicePath,
    voltage: FrontendLnbVoltage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendBackendLnbApplyOutcome {
    Applied,
    Rejected(HalError),
    Indeterminate(HalError),
}

impl FrontendBackendLnbApplyPlan {
    pub fn new(
        frontend_id: i32,
        backend: FrontendBackendKind,
        device_path: FrontendDevicePath,
        voltage: FrontendLnbVoltage,
    ) -> Self {
        Self {
            frontend_id,
            backend,
            device_path,
            voltage,
        }
    }
}

trait Px4LnbApplyOps {
    fn set_extended_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError>;
    fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError>;
}

struct RealPx4LnbApplyOps<'a> {
    fd: i32,
    path: &'a FrontendDevicePath,
}

impl<'a> Px4LnbApplyOps for RealPx4LnbApplyOps<'a> {
    fn set_extended_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError> {
        let mut requested = voltage;
        ioctl_ptr(
            "px4",
            Some(self.path.as_path().to_path_buf()),
            self.fd,
            PTXT_SET_LNB_VOLTAGE,
            &mut requested,
            "PTXT_SET_LNB_VOLTAGE",
        )
    }

    fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError> {
        if enabled {
            let result = unsafe { ptx_enable_lnb_power_scalar(self.fd, voltage as _) };
            result.map(|_| ()).map_err(|errno| HalError::IoctlFailed {
                backend: "px4",
                path: Some(self.path.as_path().to_path_buf()),
                op: "PTX_ENABLE_LNB_POWER",
                errno: errno as i32,
            })
        } else {
            ioctl_noarg(
                "px4",
                Some(self.path.as_path().to_path_buf()),
                self.fd,
                PTX_DISABLE_LNB_POWER,
                "PTX_DISABLE_LNB_POWER",
            )
        }
    }
}

pub fn apply_frontend_backend_lnb_voltage(
    plan: &FrontendBackendLnbApplyPlan,
) -> Result<(), HalError> {
    match apply_frontend_backend_lnb_voltage_classified(plan) {
        FrontendBackendLnbApplyOutcome::Applied => Ok(()),
        FrontendBackendLnbApplyOutcome::Rejected(error)
        | FrontendBackendLnbApplyOutcome::Indeterminate(error) => Err(error),
    }
}

pub fn apply_frontend_backend_lnb_voltage_classified(
    plan: &FrontendBackendLnbApplyPlan,
) -> FrontendBackendLnbApplyOutcome {
    let file = match open_rw(&plan.device_path) {
        Ok(file) => file,
        Err(error) => return FrontendBackendLnbApplyOutcome::Rejected(error),
    };
    let result = match plan.backend {
        FrontendBackendKind::Px4CharDevice => {
            let mut ops = RealPx4LnbApplyOps {
                fd: file.as_raw_fd(),
                path: &plan.device_path,
            };
            apply_px4_lnb_voltage_with_ops(&mut ops, plan.voltage)
        }
        FrontendBackendKind::LinuxDvb => {
            let mode = match dvb_lnb_voltage_mode(plan.voltage) {
                Ok(mode) => mode,
                Err(error) => return FrontendBackendLnbApplyOutcome::Rejected(error),
            };
            ioctl_word(
                "dvb",
                Some(plan.device_path.as_path().to_path_buf()),
                file.as_raw_fd(),
                FE_SET_VOLTAGE,
                mode,
                "FE_SET_VOLTAGE",
            )
        }
    };
    match result {
        Ok(()) => FrontendBackendLnbApplyOutcome::Applied,
        Err(error) => FrontendBackendLnbApplyOutcome::Indeterminate(error),
    }
}

fn apply_px4_lnb_voltage_with_ops<O: Px4LnbApplyOps>(
    ops: &mut O,
    voltage: FrontendLnbVoltage,
) -> Result<(), HalError> {
    let requested_voltage = px4_lnb_voltage_value(voltage)?;
    let extended = ops.set_extended_lnb_voltage(requested_voltage);
    let should_try_legacy = match &extended {
        Ok(()) => false,
        Err(error) => px4_lnb_voltage_fallback_allowed(error),
    };
    if !should_try_legacy {
        return extended;
    }
    let legacy_request = if requested_voltage > 0 { 2 } else { 0 };
    ops.set_legacy_lnb_enabled(requested_voltage > 0, legacy_request)
}

fn px4_lnb_voltage_value(voltage: FrontendLnbVoltage) -> Result<i32, HalError> {
    match voltage {
        FrontendLnbVoltage::None => Ok(0),
        FrontendLnbVoltage::Voltage15V => Ok(15),
        FrontendLnbVoltage::Voltage11V => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "px4 LNB backend accepts only NONE or 15V",
        )),
    }
}

fn px4_lnb_voltage_fallback_allowed(error: &HalError) -> bool {
    matches!(
        error,
        HalError::IoctlFailed { errno, .. }
            if *errno == ERRNO_ENOTTY || *errno == ERRNO_EINVAL || *errno == ERRNO_ENOSYS
    )
}

fn dvb_lnb_voltage_mode(voltage: FrontendLnbVoltage) -> Result<u32, HalError> {
    match voltage {
        FrontendLnbVoltage::None => Ok(SEC_VOLTAGE_OFF),
        FrontendLnbVoltage::Voltage11V => Ok(SEC_VOLTAGE_13),
        FrontendLnbVoltage::Voltage15V => Ok(SEC_VOLTAGE_18),
    }
}

fn initial_signal_state_from_observation(
    _frontend_id: i32,
    result: Result<FrontendSignalState, HalError>,
) -> FrontendSignalState {
    match result {
        Ok(state) => state,
        Err(_error) => FrontendSignalState::Unknown,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendBackendRollbackSnapshot {
    previous_request: Option<FrontendTuneRequest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendStreamingState {
    NotStarted,
    Started,
}

struct FrontendBackendTuneExecutor {
    plan: FrontendBackendTunePlan,
    previous_request: Option<FrontendTuneRequest>,
    kind: FrontendBackendSessionKind,
    file: Option<File>,
    streaming_state: BackendStreamingState,
    initial_signal_state: FrontendSignalState,
    px4_channel_apply_result: Option<Px4ChannelApplyResult>,
}

impl FrontendBackendTuneExecutor {
    fn open(
        plan: FrontendBackendTunePlan,
        previous_request: Option<FrontendTuneRequest>,
    ) -> Result<Self, HalError> {
        let file = open_rw(&plan.device_path)?;
        let kind = match plan.backend {
            FrontendBackendKind::Px4CharDevice => FrontendBackendSessionKind::Px4 {
                control_path: plan.device_path.clone(),
            },
            FrontendBackendKind::LinuxDvb => FrontendBackendSessionKind::Dvb {
                frontend_path: plan.device_path.clone(),
            },
        };
        Ok(Self {
            plan,
            previous_request,
            kind,
            file: Some(file),
            streaming_state: BackendStreamingState::NotStarted,
            initial_signal_state: FrontendSignalState::Unknown,
            px4_channel_apply_result: None,
        })
    }

    fn file_fd(&self) -> Result<i32, HalError> {
        self.file
            .as_ref()
            .map(|file| file.as_raw_fd())
            .ok_or_else(|| {
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "backend tune executor file was already consumed",
                )
            })
    }

    fn stop_current(&mut self) -> Result<(), HalError> {
        if self.streaming_state == BackendStreamingState::NotStarted {
            return Ok(());
        }
        let fd = self.file_fd()?;
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                px4_streaming_ioctl(control_path, fd, PTX_STOP_STREAMING, "PTX_STOP_STREAMING")
            }
            FrontendBackendSessionKind::Dvb { frontend_path } => {
                let mut prop = DtvProperty::with_data(DTV_CLEAR, 0);
                let mut props = DtvProperties {
                    num: 1,
                    props: &mut prop as *mut DtvProperty,
                };
                ioctl_ptr(
                    "dvb",
                    Some(frontend_path.as_path().to_path_buf()),
                    fd,
                    FE_SET_PROPERTY,
                    &mut props,
                    "FE_SET_PROPERTY(DTV_CLEAR)",
                )
            }
        }?;
        self.streaming_state = BackendStreamingState::NotStarted;
        Ok(())
    }

    fn apply_system_mode_for(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                let mapped = px4::map_tune_request_to_px4(request)?;
                let fd = self.file_fd()?;
                let result = unsafe { ptx_set_system_mode_scalar(fd, mapped.system_code as _) };
                result.map(|_| ()).map_err(|errno| HalError::IoctlFailed {
                    backend: "px4",
                    path: Some(control_path.as_path().to_path_buf()),
                    op: "PTX_SET_SYSTEM_MODE",
                    errno: errno as i32,
                })
            }
            // DVBはdelivery-systemとchannel propertyをFE_SET_PROPERTY(DTV_TUNE)の1回のpacketとして適用する。
            FrontendBackendSessionKind::Dvb { .. } => Ok(()),
        }
    }

    fn apply_channel_for(&mut self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                let mapped = px4::map_tune_request_to_px4(request)?;
                let mut freq = PtxFreq {
                    freq_no: mapped.freq_no,
                    slot: mapped.slot,
                };
                let result = ioctl_ptr(
                    "px4",
                    Some(control_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    PTX_SET_CHANNEL,
                    &mut freq,
                    "PTX_SET_CHANNEL",
                );
                let classified = classify_px4_channel_apply_result(request.system, result)?;
                self.px4_channel_apply_result = Some(classified);
                Ok(())
            }
            FrontendBackendSessionKind::Dvb { frontend_path } => {
                let normalized = dvb::normalized_tune_request_from_common(request)?;
                let pairs = dvb::tune_property_pairs(&normalized)?;
                let mut properties = pairs.to_dtv_properties();
                let mut props = DtvProperties {
                    num: properties.len() as u32,
                    props: properties.as_mut_ptr(),
                };
                ioctl_ptr(
                    "dvb",
                    Some(frontend_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    FE_SET_PROPERTY,
                    &mut props,
                    "FE_SET_PROPERTY(DTV_TUNE)",
                )
            }
        }
    }

    fn start_streaming_current(&mut self) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => px4_streaming_ioctl(
                control_path,
                self.file_fd()?,
                PTX_START_STREAMING,
                "PTX_START_STREAMING",
            ),
            // DVBはFE_SET_PROPERTY(DTV_TUNE)後に配送を開始するため、ここに別のuserspace start ioctlは置かない。
            FrontendBackendSessionKind::Dvb { .. } => Ok(()),
        }?;
        self.streaming_state = BackendStreamingState::Started;
        Ok(())
    }

    fn submit_request_for_rollback(
        &mut self,
        request: &FrontendTuneRequest,
    ) -> Result<(), HalError> {
        self.apply_system_mode_for(request)?;
        self.apply_channel_for(request)?;
        self.start_streaming_current()
    }

    fn initial_signal_state_after_submit(&self) -> Result<FrontendSignalState, HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                px4_signal_state_from_readback(read_px4_boolean(
                    control_path,
                    self.file_fd()?,
                    PTX_GET_LOCK_STATUS,
                    "PTX_GET_LOCK_STATUS",
                ))
            }
            FrontendBackendSessionKind::Dvb { frontend_path } => {
                let mut status: u32 = 0;
                ioctl_ptr(
                    "dvb",
                    Some(frontend_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    FE_READ_STATUS,
                    &mut status,
                    "FE_READ_STATUS",
                )?;
                if status & FE_HAS_LOCK != 0 {
                    Ok(FrontendSignalState::Locked)
                } else if status & FE_HAS_CARRIER != 0 {
                    Ok(FrontendSignalState::SignalDetected)
                } else {
                    Ok(FrontendSignalState::NoSignal)
                }
            }
        }
    }

    fn into_session(mut self) -> Result<FrontendBackendSession, HalError> {
        let file = self.file.take().ok_or_else(|| {
            HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "backend tune executor file was already consumed",
            )
        })?;
        Ok(FrontendBackendSession {
            kind: self.kind,
            file,
            initial_signal_state: self.initial_signal_state,
            partial_reception: self.plan.request.partial_reception,
            px4_channel_apply_result: self.px4_channel_apply_result,
        })
    }
}

impl BackendTuneOps for FrontendBackendTuneExecutor {
    type Snapshot = FrontendBackendRollbackSnapshot;

    fn capture_previous_state(&mut self) -> Result<Self::Snapshot, HalError> {
        Ok(FrontendBackendRollbackSnapshot {
            previous_request: self.previous_request.clone(),
        })
    }

    fn apply_system_mode(&mut self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        self.apply_system_mode_for(request)
    }

    fn apply_channel(&mut self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        self.apply_channel_for(request)
    }

    fn start_streaming(&mut self) -> Result<(), HalError> {
        self.start_streaming_current()
    }

    fn read_initial_status(&mut self) -> Result<(), HalError> {
        self.initial_signal_state = initial_signal_state_from_observation(
            self.plan.frontend_id,
            self.initial_signal_state_after_submit(),
        );
        Ok(())
    }

    fn rollback_stop_streaming(&mut self) -> Result<(), HalError> {
        self.stop_current()
    }

    fn rollback_restore_previous_state(
        &mut self,
        snapshot: &Self::Snapshot,
    ) -> Result<(), HalError> {
        match snapshot.previous_request.as_ref() {
            Some(previous) => self.submit_request_for_rollback(previous),
            None => Ok(()),
        }
    }
}

pub fn run_frontend_backend_tune_worker(
    ctx: FrontendWorkerContext,
    plan: FrontendBackendTunePlan,
) -> Result<(), HalError> {
    run_frontend_backend_tune_worker_with_previous(ctx, plan, None)
}

pub fn run_frontend_backend_tune_worker_with_previous(
    ctx: FrontendWorkerContext,
    plan: FrontendBackendTunePlan,
    previous_request: Option<FrontendTuneRequest>,
) -> Result<(), HalError> {
    plan.validate_worker_generation(ctx.generation())?;
    let session = FrontendBackendSession::open_and_submit_with_previous(&plan, previous_request)?;
    while !ctx.cancel_requested() {
        ctx.wait_until(Some(
            Instant::now()
                .checked_add(Duration::from_millis(20))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend poll deadline overflow",
                    )
                })?,
        ));
    }
    let reason = ctx.cancel_reason();
    let completion = if matches!(
        reason,
        Ok(Some(
            super::frontend_worker::FrontendWorkerCancelReason::StopRequested
        ))
    ) {
        session.stop()
    } else {
        session.close()
    };
    match (reason, completion) {
        (Ok(_), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(primary), Err(cleanup)) => Err(compose_primary_cleanup_failure(
            "frontend cancellation lookup and backend cleanup failed",
            primary,
            cleanup,
        )),
    }
}

fn open_rw(path: &FrontendDevicePath) -> Result<File, HalError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(dvb::abi::O_NONBLOCK)
        .open(path.as_path())
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => HalError::PermissionDenied {
                path: path.as_path().to_path_buf(),
                detail: HalErrorDetail::new(error.to_string()),
            },
            _ => HalError::OpenFailed {
                path: path.as_path().to_path_buf(),
                detail: HalErrorDetail::new(error.to_string()),
            },
        })
}

fn ioctl_ptr<T>(
    backend: &'static str,
    path: Option<PathBuf>,
    fd: i32,
    request: u64,
    arg: &mut T,
    op: &'static str,
) -> Result<(), HalError> {
    // 安全性: `fd` はFrontendBackendSession生成が所有し、`arg` は選択backend ABI用のC互換ioctl payloadを指す。
    let rc = unsafe { raw_ioctl_ptr(fd, request, arg) };
    if rc < 0 {
        return Err(HalError::IoctlFailed {
            backend,
            path,
            op,
            errno: last_errno(),
        });
    }
    Ok(())
}

fn read_px4_boolean(
    path: &FrontendDevicePath,
    fd: i32,
    request: u64,
    op: &'static str,
) -> Result<bool, HalError> {
    let mut value: u32 = 0;
    ioctl_ptr(
        "px4",
        Some(path.as_path().to_path_buf()),
        fd,
        request,
        &mut value,
        op,
    )?;
    decode_px4_boolean(path, op, value)
}

fn decode_px4_boolean(
    path: &FrontendDevicePath,
    op: &'static str,
    value: u32,
) -> Result<bool, HalError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HalError::Io {
            backend: "px4",
            operation: op,
            path: Some(path.as_path().to_path_buf()),
            errno: None,
            detail: HalErrorDetail::new(format!("driver returned a non-boolean value: {value}")),
        }),
    }
}

fn classify_tmcc_partial_reception_read(
    result: Result<bool, HalError>,
) -> Result<FrontendTmccPartialReceptionObservation, HalError> {
    match result {
        Ok(value) => Ok(FrontendTmccPartialReceptionObservation::Available(value)),
        Err(HalError::IoctlFailed { errno, .. }) if errno == ERRNO_EAGAIN => {
            Ok(FrontendTmccPartialReceptionObservation::Pending)
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Px4ChannelApplyResult {
    Applied,
    PendingUnlocked,
}

fn classify_px4_channel_apply_result(
    system: Option<FrontendSystem>,
    result: Result<(), HalError>,
) -> Result<Px4ChannelApplyResult, HalError> {
    match result {
        Ok(()) => Ok(Px4ChannelApplyResult::Applied),
        Err(HalError::IoctlFailed {
            errno: ERRNO_EAGAIN,
            ..
        }) if system == Some(FrontendSystem::IsdbT) => Ok(Px4ChannelApplyResult::PendingUnlocked),
        Err(error) => Err(error),
    }
}

fn px4_signal_state_from_readback(
    result: Result<bool, HalError>,
) -> Result<FrontendSignalState, HalError> {
    result.map(|locked| {
        if locked {
            FrontendSignalState::Locked
        } else {
            FrontendSignalState::NoSignal
        }
    })
}

fn px4_streaming_ioctl(
    path: &FrontendDevicePath,
    fd: i32,
    request: u64,
    op: &'static str,
) -> Result<(), HalError> {
    px4_streaming_ioctl_result(
        request,
        ioctl_noarg("px4", Some(path.as_path().to_path_buf()), fd, request, op),
    )
}

fn px4_streaming_ioctl_result(request: u64, result: Result<(), HalError>) -> Result<(), HalError> {
    match result {
        // px4_drvの停止済み応答だけを停止完了として扱う。STARTの同じerrnoは失敗のまま返す。
        Err(HalError::IoctlFailed {
            errno: ERRNO_EALREADY,
            ..
        }) if request == PTX_STOP_STREAMING => Ok(()),
        result => result,
    }
}

fn ioctl_noarg(
    backend: &'static str,
    path: Option<PathBuf>,
    fd: i32,
    request: u64,
    op: &'static str,
) -> Result<(), HalError> {
    // 安全性: 選択backend ABIに対する引数なしioctlである。
    let rc = unsafe { raw_ioctl_noarg(fd, request) };
    if rc < 0 {
        return Err(HalError::IoctlFailed {
            backend,
            path,
            op,
            errno: last_errno(),
        });
    }
    Ok(())
}

fn ioctl_word(
    backend: &'static str,
    path: Option<PathBuf>,
    fd: i32,
    request: u64,
    arg: u32,
    op: &'static str,
) -> Result<(), HalError> {
    let rc = unsafe { raw_ioctl_word(fd, request, arg) };
    if rc < 0 {
        return Err(HalError::IoctlFailed {
            backend,
            path,
            op,
            errno: last_errno(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendStreamIdKind, FrontendSystem};
    use std::thread;

    fn fresh_tune_executor(backend: FrontendBackendKind) -> FrontendBackendTuneExecutor {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let plan = FrontendBackendTunePlan::new(
            10,
            1,
            backend,
            FrontendDevicePath::new("/dev/null"),
            request,
        );
        FrontendBackendTuneExecutor::open(plan, None).unwrap()
    }

    #[test]
    fn fresh_tune_failure_does_not_stop_an_unstarted_stream() {
        for (backend, expected_step) in [
            (
                FrontendBackendKind::Px4CharDevice,
                BackendTuneStep::ApplySystemMode,
            ),
            (FrontendBackendKind::LinuxDvb, BackendTuneStep::ApplyChannel),
        ] {
            let mut executor = fresh_tune_executor(backend);
            let mut txn = BackendTuneTxn::new(10, 1, executor.plan.request.clone());
            // /dev/nullは機器要求をENOTTYで拒否する。不要なSTOPがあれば巻戻しも失敗する。
            match txn.apply(&mut executor) {
                BackendTuneOutcome::Failed {
                    step,
                    error,
                    rollback,
                } => {
                    assert_eq!(step, expected_step);
                    assert!(matches!(
                        error,
                        HalError::IoctlFailed {
                            errno: ERRNO_ENOTTY,
                            ..
                        }
                    ));
                    assert!(rollback.succeeded());
                }
                other => panic!("unexpected outcome: {other:?}"),
            }
            assert_eq!(executor.streaming_state, BackendStreamingState::NotStarted);
        }
    }

    #[test]
    fn failed_start_does_not_arm_rollback_stop() {
        let mut executor = fresh_tune_executor(FrontendBackendKind::Px4CharDevice);
        assert!(matches!(
            executor.start_streaming(),
            Err(HalError::IoctlFailed {
                errno: ERRNO_ENOTTY,
                ..
            })
        ));
        assert_eq!(executor.streaming_state, BackendStreamingState::NotStarted);
        assert!(executor.rollback_stop_streaming().is_ok());
    }

    #[test]
    fn started_executor_attempts_stop_and_preserves_state_on_failure() {
        let mut executor = fresh_tune_executor(FrontendBackendKind::LinuxDvb);
        // DVBのStartStreaming段階には別ioctlがない。実行器の成功後の状態を検査する。
        executor.start_streaming().unwrap();
        assert_eq!(executor.streaming_state, BackendStreamingState::Started);
        assert!(matches!(
            executor.rollback_stop_streaming(),
            Err(HalError::IoctlFailed {
                op: "FE_SET_PROPERTY(DTV_CLEAR)",
                errno: ERRNO_ENOTTY,
                ..
            })
        ));
        assert_eq!(executor.streaming_state, BackendStreamingState::Started);
    }

    #[test]
    fn px4_pending_result_survives_executor_commit() {
        let mut executor = fresh_tune_executor(FrontendBackendKind::Px4CharDevice);
        executor.px4_channel_apply_result = Some(Px4ChannelApplyResult::PendingUnlocked);
        let session = executor.into_session().unwrap();
        assert_eq!(
            session.px4_channel_apply_result(),
            Some(Px4ChannelApplyResult::PendingUnlocked)
        );
    }

    #[test]
    fn px4_isdbt_set_channel_eagain_is_pending_but_isdbs_remains_failure() {
        let pending = HalError::IoctlFailed {
            backend: "px4",
            path: Some(PathBuf::from("/dev/px4video0")),
            op: "PTX_SET_CHANNEL",
            errno: ERRNO_EAGAIN,
        };
        assert_eq!(
            classify_px4_channel_apply_result(Some(FrontendSystem::IsdbT), Err(pending.clone())),
            Ok(Px4ChannelApplyResult::PendingUnlocked)
        );
        assert_eq!(
            classify_px4_channel_apply_result(Some(FrontendSystem::IsdbS), Err(pending.clone())),
            Err(pending.clone())
        );
        assert_eq!(
            classify_px4_channel_apply_result(None, Err(pending.clone())),
            Err(pending)
        );
    }

    #[test]
    fn only_px4_stop_ealready_is_idempotent_success() {
        for (request, op, errno, succeeds) in [
            (
                PTX_STOP_STREAMING,
                "PTX_STOP_STREAMING",
                ERRNO_EALREADY,
                true,
            ),
            (
                PTX_STOP_STREAMING,
                "PTX_STOP_STREAMING",
                ERRNO_ENOTTY,
                false,
            ),
            (
                PTX_STOP_STREAMING,
                "PTX_STOP_STREAMING",
                ERRNO_EINVAL,
                false,
            ),
            (
                PTX_START_STREAMING,
                "PTX_START_STREAMING",
                ERRNO_EALREADY,
                false,
            ),
        ] {
            let error = HalError::IoctlFailed {
                backend: "px4",
                path: Some(PathBuf::from("/dev/px4video0")),
                op,
                errno,
            };
            let result = px4_streaming_ioctl_result(request, Err(error.clone()));
            if succeeds {
                assert_eq!(result, Ok(()));
            } else {
                assert_eq!(result, Err(error));
            }
        }
        assert_eq!(
            px4_streaming_ioctl_result(PTX_STOP_STREAMING, Ok(())),
            Ok(())
        );
    }

    #[test]
    fn dvb_live_reader_open_failure_keeps_io_context() {
        let path = FrontendDevicePath::new("/dev/null/maleicacid-tuner-dvr");
        let expected_errno = File::open(path.as_path()).unwrap_err().raw_os_error();
        let session = FrontendBackendSession {
            kind: FrontendBackendSessionKind::Dvb {
                frontend_path: FrontendDevicePath::new("/dev/null"),
            },
            file: File::open("/dev/null").unwrap(),
            initial_signal_state: FrontendSignalState::NoSignal,
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let descriptor = FrontendLiveReaderDescriptor::dvb_dvr_device(1, path.clone());
        let error = session.open_live_reader(&descriptor).err().unwrap();
        match error {
            HalError::Io {
                backend,
                operation,
                path: error_path,
                errno,
                ..
            } => {
                assert_eq!(backend, "dvb");
                assert_eq!(operation, "live dvr reader open");
                assert_eq!(error_path.as_deref(), Some(path.as_path()));
                assert!(expected_errno.is_some());
                assert_eq!(errno, expected_errno);
            }
            error => panic!("live reader open lost I/O classification: {error:?}"),
        }
    }

    #[test]
    fn px4_live_reader_uses_existing_fd_without_reopening_path() {
        let path = FrontendDevicePath::new("/dev/null/maleicacid-tuner-px4");
        let session = FrontendBackendSession {
            kind: FrontendBackendSessionKind::Px4 {
                control_path: path.clone(),
            },
            file: File::open("/dev/null").unwrap(),
            initial_signal_state: FrontendSignalState::NoSignal,
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let descriptor = FrontendLiveReaderDescriptor::px4_from_control_fd(1, path);
        let mut reader = session.open_live_reader(&descriptor).unwrap();
        assert_eq!(reader.read(&mut [0_u8; 1]).unwrap(), 0);
    }

    #[test]
    fn dvb_close_does_not_require_a_tune_stop_ioctl() {
        let session = FrontendBackendSession {
            kind: FrontendBackendSessionKind::Dvb {
                frontend_path: FrontendDevicePath::new("/dev/null"),
            },
            file: File::open("/dev/null").unwrap(),
            initial_signal_state: FrontendSignalState::NoSignal,
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        assert!(session.stop().is_err());
        assert!(session.close().is_ok());
    }

    #[test]
    fn px4_close_retains_stream_stop_failure() {
        let session = FrontendBackendSession {
            kind: FrontendBackendSessionKind::Px4 {
                control_path: FrontendDevicePath::new("/dev/null"),
            },
            file: File::open("/dev/null").unwrap(),
            initial_signal_state: FrontendSignalState::NoSignal,
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        assert!(session.close().is_err());
    }

    #[derive(Default)]
    struct FakePx4LnbOps {
        extended_result: Option<Result<(), HalError>>,
        legacy_calls: Vec<(bool, i32)>,
    }

    impl Px4LnbApplyOps for FakePx4LnbOps {
        fn set_extended_lnb_voltage(&mut self, _voltage: i32) -> Result<(), HalError> {
            self.extended_result.take().unwrap_or(Ok(()))
        }

        fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError> {
            self.legacy_calls.push((enabled, voltage));
            Ok(())
        }
    }

    #[test]
    fn px4_boolean_readback_rejects_non_boolean_driver_values() {
        let path = FrontendDevicePath::new("/dev/px4video0");
        assert_eq!(decode_px4_boolean(&path, "TEST", 0), Ok(false));
        assert_eq!(decode_px4_boolean(&path, "TEST", 1), Ok(true));
        assert!(matches!(
            decode_px4_boolean(&path, "TEST", 2),
            Err(HalError::Io {
                backend: "px4",
                operation: "TEST",
                errno: None,
                ..
            })
        ));
    }

    #[test]
    fn px4_demod_lock_readback_distinguishes_unlocked_from_io_failure() {
        assert_eq!(
            px4_signal_state_from_readback(Ok(false)),
            Ok(FrontendSignalState::NoSignal)
        );
        assert_eq!(
            px4_signal_state_from_readback(Ok(true)),
            Ok(FrontendSignalState::Locked)
        );
        let failure = px4_signal_state_from_readback(Err(HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTX_GET_LOCK_STATUS",
            errno: 5,
        }));
        assert!(matches!(
            failure,
            Err(HalError::IoctlFailed { errno: 5, .. })
        ));
    }

    #[test]
    fn tmcc_eagain_is_pending_and_other_ioctls_remain_failures() {
        let pending = classify_tmcc_partial_reception_read(Err(HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTX_GET_TMCC_PARTIAL_RECEPTION",
            errno: ERRNO_EAGAIN,
        }));
        assert_eq!(
            pending,
            Ok(FrontendTmccPartialReceptionObservation::Pending)
        );

        for errno in [5, ERRNO_ENOTTY] {
            let failure = classify_tmcc_partial_reception_read(Err(HalError::IoctlFailed {
                backend: "px4",
                path: None,
                op: "PTX_GET_TMCC_PARTIAL_RECEPTION",
                errno,
            }));
            assert!(matches!(
                failure,
                Err(HalError::IoctlFailed {
                    errno: observed,
                    ..
                }) if observed == errno
            ));
        }
    }

    #[test]
    fn tune_plan_keeps_backend_path_and_request() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let plan = FrontendBackendTunePlan::new(
            7,
            41,
            FrontendBackendKind::LinuxDvb,
            FrontendDevicePath::new("/dev/dvb/adapter0/frontend0"),
            request.clone(),
        );
        assert_eq!(plan.frontend_id, 7);
        assert_eq!(plan.generation, 41);
        assert_eq!(plan.backend, FrontendBackendKind::LinuxDvb);
        assert_eq!(plan.device_path.display(), "/dev/dvb/adapter0/frontend0");
        assert_eq!(plan.request, request);
    }

    #[test]
    fn px4_satellite_plan_retains_stream_selector() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let plan = FrontendBackendTunePlan::new(
            8,
            42,
            FrontendBackendKind::Px4CharDevice,
            FrontendDevicePath::new("/dev/px4video0"),
            request.clone(),
        );
        assert!(matches!(plan.backend, FrontendBackendKind::Px4CharDevice));
        assert_eq!(plan.request.stream_id, Some(0x4010));
    }

    #[test]
    fn tune_plan_detects_worker_generation_mismatch() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let plan = FrontendBackendTunePlan::new(
            9,
            55,
            FrontendBackendKind::LinuxDvb,
            FrontendDevicePath::new("/dev/dvb/adapter0/frontend0"),
            request,
        );
        assert!(plan.validate_worker_generation(55).is_ok());
        assert!(matches!(
            plan.validate_worker_generation(56),
            Err(HalError::Internal { .. })
        ));
    }

    #[test]
    fn backend_tune_txn_uses_plan_generation() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let plan = FrontendBackendTunePlan::new(
            10,
            77,
            FrontendBackendKind::LinuxDvb,
            FrontendDevicePath::new("/dev/dvb/adapter0/frontend0"),
            request.clone(),
        );
        let txn = BackendTuneTxn::new(plan.frontend_id, plan.generation, request);
        assert_eq!(txn.generation(), plan.generation);
    }

    #[test]
    fn submit_failure_preserves_original_error_kind() {
        let failure = FrontendBackendSubmitFailure {
            generation: 99,
            error: HalError::IoctlFailed {
                backend: "dvb",
                path: None,
                op: "FE_SET_PROPERTY",
                errno: 5,
            },
            rollback_succeeded: false,
            step: Some(BackendTuneStep::ApplyChannel),
            rollback_failure: None,
        };
        let error = failure.into_error();
        assert!(matches!(
            error.primary_error(),
            HalError::IoctlFailed {
                backend: "dvb",
                op: "FE_SET_PROPERTY",
                errno: 5,
                ..
            }
        ));
        assert!(matches!(error, HalError::ComposedFailure { .. }));
    }

    #[test]
    fn submit_deadline_retains_cleanup_ownership_until_thread_exit() {
        let generation = 100;
        let ticket = FrontendBackendSubmitTicket::start_with(generation, move || {
            thread::sleep(Duration::from_millis(25));
            Err(FrontendBackendSubmitFailure {
                generation,
                error: HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "simulated delayed submit failure",
                ),
                rollback_succeeded: true,
                step: None,
                rollback_failure: None,
            })
        })
        .unwrap();

        let mut ticket = match ticket
            .wait_until(Instant::now() + Duration::from_millis(1))
            .unwrap()
        {
            FrontendBackendSubmitWait::TimedOut(ticket) => ticket,
            FrontendBackendSubmitWait::Completed(_) => {
                panic!("delayed backend submit completed before its test deadline")
            }
        };

        assert!(ticket
            .wait_until_cleanup(Some(Instant::now() + Duration::from_secs(1)))
            .unwrap());
        let failure = ticket.complete_cleanup().unwrap_err();
        assert_eq!(failure.generation, generation);
        assert!(failure.rollback_succeeded);
        assert!(failure.cleanup_result().is_ok());
        assert_eq!(
            failure.error,
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "simulated delayed submit failure",
            )
        );
    }

    #[test]
    fn delayed_submit_keeps_primary_and_rollback_failure_in_both_cleanup_paths() {
        for poll in [false, true] {
            let (release, wait) = mpsc::sync_channel(1);
            let expected = FrontendBackendSubmitFailure {
                generation: 101,
                error: HalError::IoctlFailed {
                    backend: "px4",
                    path: None,
                    op: "set channel",
                    errno: 5,
                },
                rollback_succeeded: false,
                step: Some(BackendTuneStep::ApplyChannel),
                rollback_failure: Some(super::super::tune_txn::BackendTuneRollbackFailure {
                    step: super::super::tune_txn::BackendTuneRollbackStep::RollbackStopStreaming,
                    error: HalError::IoctlFailed {
                        backend: "px4",
                        path: None,
                        op: "stop streaming",
                        errno: 16,
                    },
                }),
            };
            let failure = expected.clone();
            let ticket = FrontendBackendSubmitTicket::start_with(101, move || {
                wait.recv().unwrap();
                Err(failure)
            })
            .unwrap();
            let mut ticket = match ticket.wait_until(Instant::now()).unwrap() {
                FrontendBackendSubmitWait::TimedOut(ticket) => ticket,
                _ => panic!("blocked submit must time out"),
            };
            release.send(()).unwrap();
            assert!(ticket
                .wait_until_cleanup(Some(Instant::now() + Duration::from_secs(1)))
                .unwrap());
            let result = if poll {
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    if let Some(result) = ticket.try_complete_cleanup() {
                        break result;
                    }
                    assert!(Instant::now() < deadline, "submit thread did not exit");
                    thread::yield_now();
                }
            } else {
                ticket.complete_cleanup()
            };
            let failure = result.unwrap_err();
            assert_eq!(failure, expected);
            assert!(failure.cleanup_result().is_err());
        }
    }

    #[test]
    fn lost_backend_cleanup_ticket_keeps_the_submit_owner_in_the_registry() {
        use crate::{
            FrontendWorkerCancelReason, FrontendWorkerKind, FrontendWorkerRegistry,
            FrontendWorkerStopOutcome,
        };

        let (release_tx, release_rx) = mpsc::channel();
        let ticket = FrontendBackendSubmitTicket::start_with(101, move || {
            release_rx.recv().unwrap();
            Err(FrontendBackendSubmitFailure {
                generation: 101,
                error: HalError::cleanup_failed("submit test", "rejected before side effects"),
                rollback_succeeded: true,
                step: None,
                rollback_failure: None,
            })
        })
        .unwrap();
        let mut registry = FrontendWorkerRegistry::default();
        let first =
            registry.retain_backend_submit_cleanup(1, FrontendWorkerKind::Tune, 101, ticket);
        std::mem::forget(first);
        assert!(registry.has_cleanup_obligations());
        let next = registry.request_stop_for_join(
            1,
            FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested,
        );
        release_tx.send(()).unwrap();
        assert!(matches!(
            next.complete(),
            FrontendWorkerStopOutcome::BackendSubmitFailed {
                generation: 101,
                failure: FrontendBackendSubmitFailure {
                    rollback_succeeded: true,
                    ..
                },
                ..
            }
        ));
        assert!(!registry.has_cleanup_obligations());
    }

    #[test]
    fn failed_backend_rollback_keeps_cleanup_pending_after_join() {
        use crate::{
            FrontendWorkerCancelReason, FrontendWorkerKind, FrontendWorkerRegistry,
            FrontendWorkerStopOutcome,
        };
        let expected = FrontendBackendSubmitFailure {
            generation: 102,
            error: HalError::cleanup_failed("backend", "submit failed"),
            rollback_succeeded: false,
            step: Some(BackendTuneStep::ApplyChannel),
            rollback_failure: Some(super::super::tune_txn::BackendTuneRollbackFailure {
                step: super::super::tune_txn::BackendTuneRollbackStep::RollbackStopStreaming,
                error: HalError::cleanup_failed("backend", "stop failed"),
            }),
        };
        let failure = expected.clone();
        let ticket = FrontendBackendSubmitTicket::start_with(102, move || Err(failure)).unwrap();
        let mut registry = FrontendWorkerRegistry::default();
        let ticket =
            registry.retain_backend_submit_cleanup(1, FrontendWorkerKind::Tune, 102, ticket);
        assert!(
            matches!(ticket.complete(), FrontendWorkerStopOutcome::BackendSubmitFailed { failure, .. } if failure == expected)
        );
        assert!(registry.has_cleanup_obligations());
        assert!(matches!(
            registry
                .request_stop_for_join(
                    1,
                    FrontendWorkerKind::Tune,
                    FrontendWorkerCancelReason::StopRequested
                )
                .complete(),
            FrontendWorkerStopOutcome::StopRequestFailed {
                error: HalError::WorkerCleanupFailed {
                    kind: maleicacid_tuner_hal2_common::WorkerCleanupFailureKind::Quarantined
                },
                ..
            }
        ));
    }

    #[test]
    fn dvb_initial_status_observation_failure_falls_back_to_unknown_signal() {
        let state = initial_signal_state_from_observation(
            9,
            Err(HalError::IoctlFailed {
                backend: "dvb",
                path: None,
                op: "FE_READ_STATUS",
                errno: 5,
            }),
        );
        assert_eq!(state, FrontendSignalState::Unknown);
    }

    #[test]
    fn px4_lnb_voltage_uses_legacy_fallback_for_old_driver_ioctl() {
        let mut ops = FakePx4LnbOps {
            extended_result: Some(Err(HalError::IoctlFailed {
                backend: "px4",
                path: None,
                op: "PTXT_SET_LNB_VOLTAGE",
                errno: ERRNO_ENOTTY,
            })),
            legacy_calls: Vec::new(),
        };

        apply_px4_lnb_voltage_with_ops(&mut ops, FrontendLnbVoltage::Voltage15V)
            .expect("legacy fallback succeeds");

        assert_eq!(ops.legacy_calls, vec![(true, 2)]);
    }

    #[test]
    fn px4_lnb_voltage_rejects_11v_before_ioctl() {
        let mut ops = FakePx4LnbOps::default();

        let error =
            apply_px4_lnb_voltage_with_ops(&mut ops, FrontendLnbVoltage::Voltage11V).unwrap_err();

        assert!(matches!(error, HalError::InvalidArgument { .. }));
        assert!(ops.legacy_calls.is_empty());
    }

    #[test]
    fn dvb_lnb_voltage_maps_fixed_profile_modes() {
        assert_eq!(
            dvb_lnb_voltage_mode(FrontendLnbVoltage::None).unwrap(),
            SEC_VOLTAGE_OFF
        );
        assert_eq!(
            dvb_lnb_voltage_mode(FrontendLnbVoltage::Voltage11V).unwrap(),
            SEC_VOLTAGE_13
        );
        assert_eq!(
            dvb_lnb_voltage_mode(FrontendLnbVoltage::Voltage15V).unwrap(),
            SEC_VOLTAGE_18
        );
    }
}
