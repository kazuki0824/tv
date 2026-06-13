use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use maleicacid_tuner_hal2_common::os_abi::{ioctl, last_errno};
use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendTuneRequest, HalError, HalErrorDetail,
    HalInternalKind,
};

use super::reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
use super::tune_txn::{BackendTuneOps, BackendTuneOutcome, BackendTuneStep, BackendTuneTxn};
use crate::dvb;
use crate::dvb::abi::{
    DtvProperties, DtvProperty, DTV_CLEAR, FE_HAS_LOCK, FE_HAS_SIGNAL, FE_READ_STATUS,
    FE_SET_PROPERTY,
};
use crate::px4;
use crate::px4::abi::{
    PtxFreq, PTX_GET_CNR, PTX_SET_CHANNEL, PTX_SET_SYSTEM_MODE, PTX_START_STREAMING,
    PTX_STOP_STREAMING,
};
use crate::runtime::{FrontendSignalState, FrontendWorkerContext};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendBackendTunePlan {
    pub frontend_id: i32,
    pub generation: u64,
    pub backend: FrontendBackendKind,
    pub device_path: FrontendDevicePath,
    pub request: FrontendTuneRequest,
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
    pub fn into_error(self) -> HalError {
        self.error
    }
}

fn initial_signal_state_from_observation(
    frontend_id: i32,
    result: Result<FrontendSignalState, HalError>,
) -> FrontendSignalState {
    match result {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "maleicacid-tuner-hal2-backend-readiness: frontend_id={} step={:?} error={:?}",
                frontend_id,
                BackendTuneStep::ReadInitialStatus,
                error,
            );
            FrontendSignalState::Unknown
        }
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

    fn read_signal_state(&self) -> Result<FrontendSignalState, HalError> {
        match &self.kind {
            FrontendBackendSessionKind::Px4 { control_path } => {
                let mut cnr: u32 = 0;
                ioctl_ptr(
                    "px4",
                    Some(control_path.as_path().to_path_buf()),
                    self.file_fd()?,
                    PTX_GET_CNR,
                    &mut cnr,
                    "PTX_GET_CNR",
                )?;
                if cnr == 0 {
                    Ok(FrontendSignalState::NoSignal)
                } else {
                    Ok(FrontendSignalState::SignalDetected)
                }
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
        self.initial_signal_state =
            initial_signal_state_from_observation(self.plan.frontend_id, self.read_signal_state());
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

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendStreamIdKind, FrontendSystem};

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
        assert!(matches!(
            failure.into_error(),
            HalError::IoctlFailed {
                backend: "dvb",
                op: "FE_SET_PROPERTY",
                errno: 5,
                ..
            }
        ));
    }

    #[test]
    fn initial_status_observation_failure_falls_back_to_unknown_signal() {
        let state = initial_signal_state_from_observation(
            9,
            Err(HalError::IoctlFailed {
                backend: "px4",
                path: None,
                op: "PTX_GET_CNR",
                errno: 5,
            }),
        );
        assert_eq!(state, FrontendSignalState::Unknown);
    }
}
