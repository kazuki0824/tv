use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, SyncSender, TrySendError,
};
use std::thread;
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::os_abi::{ioctl, last_errno};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendBackendKind, FrontendDevicePath, FrontendTuneRequest,
    HalError, HalErrorDetail, HalInternalKind, HalInvalidArgumentKind,
};

use super::reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
use super::thread_result_owner::{ThreadResultOwner, ThreadResultPoll};
use super::tune_txn::{BackendTuneOps, BackendTuneOutcome, BackendTuneStep, BackendTuneTxn};
use crate::dvb;
use crate::dvb::abi::{
    DtvProperties, DtvProperty, DTV_CLEAR, FE_HAS_LOCK, FE_HAS_SIGNAL, FE_READ_STATUS,
    FE_SET_PROPERTY, FE_SET_VOLTAGE, SEC_VOLTAGE_13, SEC_VOLTAGE_18, SEC_VOLTAGE_OFF,
};
use crate::px4;
use crate::px4::abi::{
    PtxFreq, ERRNO_EINVAL, ERRNO_ENOSYS, ERRNO_ENOTTY, PTXT_SET_LNB_VOLTAGE, PTX_DISABLE_LNB_POWER,
    PTX_ENABLE_LNB_POWER, PTX_SET_CHANNEL, PTX_SET_SYSTEM_MODE, PTX_START_STREAMING,
    PTX_STOP_STREAMING,
};
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

pub struct FrontendBackendSession {
    kind: FrontendBackendSessionKind,
    file: File,
    initial_signal_state: FrontendSignalState,
}

impl core::fmt::Debug for FrontendBackendSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrontendBackendSession")
            .field("kind", &self.kind)
            .field("fd", &self.file.as_raw_fd())
            .field("initial_signal_state", &self.initial_signal_state)
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
            }),
            BackendTuneOutcome::RollbackFailed {
                step,
                error,
                rollback: _,
            } => Err(FrontendBackendSubmitFailure {
                generation: plan.generation,
                error,
                rollback_succeeded: false,
                step: Some(step),
            }),
        }
    }

    pub fn initial_signal_state(&self) -> FrontendSignalState {
        self.initial_signal_state
    }

    pub fn observe_signal_state(&self) -> Result<FrontendSignalState, HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { .. } => Err(HalError::Unsupported(
                "px4 current demodulator lock readback is unavailable",
            )),
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
                } else if status & FE_HAS_SIGNAL != 0 {
                    Ok(FrontendSignalState::SignalDetected)
                } else {
                    Ok(FrontendSignalState::NoSignal)
                }
            }
        }
    }

    pub fn open_live_reader(
        &self,
        descriptor: &FrontendLiveReaderDescriptor,
    ) -> Result<Box<dyn Read + Send>, HalError> {
        match (&self.kind, &descriptor.kind) {
            (
                FrontendBackendSessionKind::Px4 { .. },
                FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { .. },
            ) => {
                let file = self.file.try_clone().map_err(|error| {
                    HalError::cleanup_failed("px4 live reader fd duplication", error.to_string())
                })?;
                Ok(Box::new(file))
            }
            (
                FrontendBackendSessionKind::Dvb { .. },
                FrontendLiveReaderDescriptorKind::DvbDvrDevice { dvr_path },
            ) => {
                let file = File::open(dvr_path.as_path()).map_err(|error| {
                    HalError::cleanup_failed(
                        "dvb live dvr reader open",
                        format!("{}: {error}", dvr_path.display()),
                    )
                })?;
                Ok(Box::new(file))
            }
            _ => Err(HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "frontend backend session and live reader descriptor kind mismatch",
            )),
        }
    }

    pub fn stop(&self) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => ioctl_noarg(
                "px4",
                Some(control_path.as_path().to_path_buf()),
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
}

impl FrontendBackendSubmitFailure {
    pub fn indeterminate(generation: u64, error: HalError) -> Self {
        frontend_backend_submit_thread_failure(generation, error)
    }

    pub fn into_error(self) -> HalError {
        let Some(step) = self.step else {
            return self.error;
        };
        let rollback_detail = if self.rollback_succeeded {
            "rollback succeeded"
        } else {
            "rollback failed"
        };
        compose_primary_cleanup_failure(
            "frontend backend submit failure",
            self.error,
            HalError::cleanup_failed(
                "frontend backend tune transaction",
                format!(
                    "generation={} step={step:?} {rollback_detail}",
                    self.generation
                ),
            ),
        )
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
pub struct FrontendBackendSubmitTicket {
    generation: u64,
    ready: Receiver<FrontendBackendSubmitReady>,
    disposition: SyncSender<FrontendBackendSubmitDisposition>,
    owner: Option<ThreadResultOwner<FrontendBackendSubmitThreadOutcome>>,
}

#[derive(Debug)]
pub enum FrontendBackendSubmitWait {
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
            FrontendBackendSession::open_and_submit_with_previous_report(
                &plan,
                previous_request,
            )
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
                        return Ok(FrontendBackendSubmitThreadOutcome::Aborted(session.stop()));
                    }
                    match disposition_receiver.recv() {
                        Ok(FrontendBackendSubmitDisposition::Claim) => {
                            Ok(FrontendBackendSubmitThreadOutcome::Claimed(session))
                        }
                        Ok(FrontendBackendSubmitDisposition::Abort) | Err(_) => {
                            Ok(FrontendBackendSubmitThreadOutcome::Aborted(session.stop()))
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

    pub fn wait_until(
        mut self,
        deadline: Instant,
    ) -> Result<FrontendBackendSubmitWait, HalError> {
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
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => Ok(
                        FrontendBackendSubmitWait::Completed(Ok(session)),
                    ),
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => Ok(
                        FrontendBackendSubmitWait::Completed(Err(failure)),
                    ),
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
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => Ok(
                        FrontendBackendSubmitWait::Completed(Err(failure)),
                    ),
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => {
                        let stop_result = session.stop();
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
                    FrontendBackendSubmitThreadOutcome::Failed(failure) => Ok(
                        FrontendBackendSubmitWait::Completed(Err(failure)),
                    ),
                    FrontendBackendSubmitThreadOutcome::Claimed(session) => {
                        let stop_error = session.stop().err().unwrap_or_else(|| {
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

    pub(crate) fn try_complete_cleanup(&mut self) -> Option<Result<(), HalError>> {
        let outcome = match self.owner.as_mut()?.collect_if_finished() {
            ThreadResultPoll::Running => return None,
            ThreadResultPoll::Completed(outcome) => outcome,
        };
        self.owner = None;
        Some(frontend_backend_submit_cleanup_result(outcome))
    }

    pub(crate) fn complete_cleanup(mut self) -> Result<(), HalError> {
        let owner = self.owner.take().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend backend submit cleanup owner is missing",
            )
        })?;
        frontend_backend_submit_cleanup_result(owner.join_after_stop())
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
        if let Some(owner) = self.owner.take() {
            // 明示claimまたはReaper移管を通らないDropでJoinHandleをdetachしない。
            // Abort後のendpoint所有権をthreadへ残し、process lifetime中の再利用を防ぐ。
            core::mem::forget(owner);
        }
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
    }
}

fn frontend_backend_submit_cleanup_result(
    outcome: Result<FrontendBackendSubmitThreadOutcome, HalError>,
) -> Result<(), HalError> {
    match outcome {
        Ok(FrontendBackendSubmitThreadOutcome::Aborted(stop_result)) => stop_result,
        Ok(FrontendBackendSubmitThreadOutcome::Failed(failure)) => {
            if failure.rollback_succeeded {
                Ok(())
            } else {
                Err(failure.into_error())
            }
        }
        Ok(FrontendBackendSubmitThreadOutcome::Claimed(session)) => {
            let stop_result = session.stop();
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
    }
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
            let mut requested = voltage;
            ioctl_ptr(
                "px4",
                Some(self.path.as_path().to_path_buf()),
                self.fd,
                PTX_ENABLE_LNB_POWER,
                &mut requested,
                "PTX_ENABLE_LNB_POWER",
            )
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
            let mode = dvb_lnb_voltage_mode(plan.voltage)?;
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

struct FrontendBackendTuneExecutor {
    plan: FrontendBackendTunePlan,
    previous_request: Option<FrontendTuneRequest>,
    kind: FrontendBackendSessionKind,
    file: Option<File>,
    initial_signal_state: FrontendSignalState,
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
            initial_signal_state: FrontendSignalState::Unknown,
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

    fn stop_current(&self) -> Result<(), HalError> {
        let fd = self.file_fd()?;
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => ioctl_noarg(
                "px4",
                Some(control_path.as_path().to_path_buf()),
                fd,
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
                    fd,
                    FE_SET_PROPERTY,
                    &mut props,
                    "FE_SET_PROPERTY(DTV_CLEAR)",
                )
            }
        }
    }

    fn apply_system_mode_for(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                let mapped = px4::map_tune_request_to_px4(request)?;
                let mut system = mapped.system_code;
                ioctl_ptr(
                    "px4",
                    Some(control_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    PTX_SET_SYSTEM_MODE,
                    &mut system,
                    "PTX_SET_SYSTEM_MODE",
                )
            }
            // DVBはdelivery-systemとchannel propertyをFE_SET_PROPERTY(DTV_TUNE)の1回のpacketとして適用する。
            FrontendBackendSessionKind::Dvb { .. } => Ok(()),
        }
    }

    fn apply_channel_for(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                let mapped = px4::map_tune_request_to_px4(request)?;
                let mut freq = PtxFreq {
                    freq_no: mapped.freq_no,
                    slot: mapped.slot,
                };
                ioctl_ptr(
                    "px4",
                    Some(control_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    PTX_SET_CHANNEL,
                    &mut freq,
                    "PTX_SET_CHANNEL",
                )
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

    fn start_streaming_current(&self) -> Result<(), HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => ioctl_noarg(
                "px4",
                Some(control_path.as_path().to_path_buf()),
                self.file_fd()?,
                PTX_START_STREAMING,
                "PTX_START_STREAMING",
            ),
            // DVBはFE_SET_PROPERTY(DTV_TUNE)後に配送を開始するため、ここに別のuserspace start ioctlは置かない。
            FrontendBackendSessionKind::Dvb { .. } => Ok(()),
        }
    }

    fn submit_request_for_rollback(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        self.apply_system_mode_for(request)?;
        self.apply_channel_for(request)?;
        self.start_streaming_current()
    }

    fn initial_signal_state_after_submit(&self) -> Result<FrontendSignalState, HalError> {
        match &self.kind {
            // PTX_SET_CHANNEL成功はdriverがdemod lockを確認した一回限りの証跡である。
            // 選局後のcurrent lock readbackとして保持しない。
            FrontendBackendSessionKind::Px4 { .. } => Ok(FrontendSignalState::Locked),
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
                } else if status & FE_HAS_SIGNAL != 0 {
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

    fn stop_previous_tune(&mut self) -> Result<(), HalError> {
        self.stop_current()
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
        thread::sleep(Duration::from_millis(20));
    }
    session.stop()
}

fn open_rw(path: &FrontendDevicePath) -> Result<File, HalError> {
    OpenOptions::new()
        .read(true)
        .write(true)
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
    let rc = unsafe { ioctl(fd, request, arg) };
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

fn ioctl_noarg(
    backend: &'static str,
    path: Option<PathBuf>,
    fd: i32,
    request: u64,
    op: &'static str,
) -> Result<(), HalError> {
    // 安全性: 選択backend ABIに対する引数なしioctlである。
    let rc = unsafe { ioctl(fd, request) };
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
    let rc = unsafe { ioctl(fd, request, arg) };
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
    fn tune_plan_keeps_backend_path_and_request() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
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
            })
        })
        .unwrap();

        let ticket = match ticket
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
        assert!(ticket.complete_cleanup().is_ok());
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
