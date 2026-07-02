use std::path::PathBuf;

use maleicacid_tuner_hal2_common::{FrontendBackendKind, HalError};
use maleicacid_tuner_hal2_demux::PacketPid;
use maleicacid_tuner_hal2_descrambler::DescramblerPid;
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub const DEFAULT_DIAGNOSTIC_STORE_LIMIT: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedDiagnosticStore<T> {
    records: Vec<T>,
    dropped_count: u64,
    limit: usize,
}

impl<T> BoundedDiagnosticStore<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            records: Vec::new(),
            dropped_count: 0,
            limit,
        }
    }

    pub fn push(&mut self, record: T) {
        if self.limit == 0 {
            self.dropped_count = self.dropped_count.saturating_add(1);
            return;
        }
        if self.records.len() >= self.limit {
            self.records.remove(0);
            self.dropped_count = self.dropped_count.saturating_add(1);
        }
        self.records.push(record);
    }

    pub fn as_slice(&self) -> &[T] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.dropped_count = 0;
    }
}

impl<T> Default for BoundedDiagnosticStore<T> {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_STORE_LIMIT)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticKind {
    DeviceMissing,
    DeviceOpenFailed,
    CapabilitySuppressed,
    DuplicateFrontendId,
    DuplicateLnbId,
    RuntimeDispatchMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticPhase {
    ProbeDevice,
    OpenDevice,
    CapabilityFilter,
    RegistryCommit,
    DispatchValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilitySuppressionReason {
    UnsupportedDeliverySystem,
    DeviceFamilyDisabled,
    NoExportableFrontend,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupDiagnosticRecord {
    pub kind: StartupDiagnosticKind,
    pub phase: StartupDiagnosticPhase,
    pub backend: Option<FrontendBackendKind>,
    pub path: Option<PathBuf>,
    pub error: Option<HalError>,
    pub capability_reason: Option<CapabilitySuppressionReason>,
}

impl StartupDiagnosticRecord {
    pub fn device_missing(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            kind: StartupDiagnosticKind::DeviceMissing,
            phase: StartupDiagnosticPhase::ProbeDevice,
            backend: Some(backend),
            path: Some(path.clone()),
            error: Some(HalError::DeviceMissing(path)),
            capability_reason: None,
        }
    }

    pub fn device_open_failed(
        backend: FrontendBackendKind,
        path: impl Into<PathBuf>,
        error: HalError,
    ) -> Self {
        Self {
            kind: StartupDiagnosticKind::DeviceOpenFailed,
            phase: StartupDiagnosticPhase::OpenDevice,
            backend: Some(backend),
            path: Some(path.into()),
            error: Some(error),
            capability_reason: None,
        }
    }

    pub fn capability_suppressed(
        backend: FrontendBackendKind,
        path: impl Into<PathBuf>,
        reason: CapabilitySuppressionReason,
    ) -> Self {
        Self {
            kind: StartupDiagnosticKind::CapabilitySuppressed,
            phase: StartupDiagnosticPhase::CapabilityFilter,
            backend: Some(backend),
            path: Some(path.into()),
            error: None,
            capability_reason: Some(reason),
        }
    }

    pub fn duplicate_frontend_id(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        Self {
            kind: StartupDiagnosticKind::DuplicateFrontendId,
            phase: StartupDiagnosticPhase::RegistryCommit,
            backend: Some(backend),
            path: Some(path.into()),
            error: None,
            capability_reason: None,
        }
    }

    pub fn duplicate_lnb_id(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        Self {
            kind: StartupDiagnosticKind::DuplicateLnbId,
            phase: StartupDiagnosticPhase::RegistryCommit,
            backend: Some(backend),
            path: Some(path.into()),
            error: None,
            capability_reason: None,
        }
    }

    pub fn runtime_dispatch_missing() -> Self {
        Self {
            kind: StartupDiagnosticKind::RuntimeDispatchMissing,
            phase: StartupDiagnosticPhase::DispatchValidation,
            backend: None,
            path: None,
            error: None,
            capability_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOpenRollbackKind {
    ObjectRegistrationRollbackFailed,
    RuntimeCleanupMissing,
    BothFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOpenRollbackPhase {
    FilterOpen,
    DvrOpen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildOpenRollbackDiagnosticRecord {
    pub kind: ChildOpenRollbackKind,
    pub phase: ChildOpenRollbackPhase,
    pub object_kind: AidlObjectKind,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub runtime_id: i32,
    pub object_error: Option<HalError>,
    pub runtime_cleanup_error: Option<HalError>,
}

impl ChildOpenRollbackDiagnosticRecord {
    pub fn new(
        phase: ChildOpenRollbackPhase,
        kind: ChildOpenRollbackKind,
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        runtime_id: i32,
        object_error: Option<HalError>,
        runtime_cleanup_error: Option<HalError>,
    ) -> Self {
        Self {
            kind,
            phase,
            object_kind,
            object_id,
            generation,
            runtime_id,
            object_error,
            runtime_cleanup_error,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrPostCommitNotificationPhase {
    InitialStatusDelivery,
    StatusNotifierStart,
    StatusNotifierStop,
    StatusNotifierRuntimeFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrPostCommitNotificationDiagnosticRecord {
    pub phase: DvrPostCommitNotificationPhase,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub error: HalError,
}

impl DvrPostCommitNotificationDiagnosticRecord {
    pub fn new(
        phase: DvrPostCommitNotificationPhase,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        error: HalError,
    ) -> Self {
        Self {
            phase,
            object_id,
            generation,
            error,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerDiagnosticKind {
    KeyTokenEmpty,
    KeyTokenInvalidLength,
    KeyTokenUnknown,
    KeyTokenExpired,
    CasTokenProducerUnavailable,
    SessionClosed,
    KeyTokenReleaseFailed,
    ClearKeyPlanMismatch,
    ReplaceKeyPlanMismatch,
    PidClaimRejected,
    PacketDescrambled,
    PacketScrambledWithoutKey,
    PacketAssemblySuppressed,
    PacketDescrambleFailed,
    InvalidPacketSize,
    BadSyncByte,
    InvalidAfc,
    InvalidAdaptationField,
    InvalidTsc,
    TransportErrorRecord,
    ScrambledNullPid,
    ScrambledWithoutPayload,
    BadToken,
    Multi2Fail,
    ScrambledWithoutDescrambler,
    PacketSourceFilterInvalid,
    PacketSourceFilterGenerationMismatch,
    CleanupKeyReleaseFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerDiagnosticPhase {
    SetKeyToken,
    AddPid,
    RemovePid,
    PacketPipeline,
    Cleanup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DescramblerDiagnosticRecord {
    SetKeyTokenFailure {
        descrambler_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    },
    PidClaimRejected {
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        demux_id: i32,
        pid: DescramblerPid,
        filter_id: i32,
        error: HalError,
    },
    PidClaimRejectedWithoutDemux {
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        pid: DescramblerPid,
        filter_id: i32,
        error: HalError,
    },
    PidClaimRejectedInvalidPid {
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        demux_id: i32,
        input_pid: u16,
        filter_id: i32,
        error: HalError,
    },
    PidClaimRejectedInvalidPidWithoutDemux {
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        input_pid: u16,
        filter_id: i32,
        error: HalError,
    },
    PacketPolicy {
        demux_id: i32,
        pid: PacketPid,
        kind: DescramblerDiagnosticKind,
    },
    PacketPolicyWithoutPid {
        demux_id: i32,
        kind: DescramblerDiagnosticKind,
    },
    PacketSourceFilterValidation {
        demux_id: i32,
        pid: PacketPid,
        filter_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    },
    CleanupKeyReleaseFailed {
        descrambler_id: i32,
        error: HalError,
    },
}

impl DescramblerDiagnosticRecord {
    pub fn set_key_token(
        descrambler_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    ) -> Self {
        Self::SetKeyTokenFailure {
            descrambler_id,
            kind,
            error,
        }
    }

    pub fn pid_claim_with_demux(
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        demux_id: i32,
        pid: DescramblerPid,
        filter_id: i32,
        error: HalError,
    ) -> Self {
        Self::PidClaimRejected {
            phase,
            descrambler_id,
            demux_id,
            pid,
            filter_id,
            error,
        }
    }

    pub fn pid_claim_without_demux(
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        pid: DescramblerPid,
        filter_id: i32,
        error: HalError,
    ) -> Self {
        Self::PidClaimRejectedWithoutDemux {
            phase,
            descrambler_id,
            pid,
            filter_id,
            error,
        }
    }

    pub fn pid_claim_invalid_pid_with_demux(
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        demux_id: i32,
        input_pid: u16,
        filter_id: i32,
        error: HalError,
    ) -> Self {
        Self::PidClaimRejectedInvalidPid {
            phase,
            descrambler_id,
            demux_id,
            input_pid,
            filter_id,
            error,
        }
    }

    pub fn pid_claim_invalid_pid_without_demux(
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        input_pid: u16,
        filter_id: i32,
        error: HalError,
    ) -> Self {
        Self::PidClaimRejectedInvalidPidWithoutDemux {
            phase,
            descrambler_id,
            input_pid,
            filter_id,
            error,
        }
    }

    pub fn packet_policy(demux_id: i32, pid: PacketPid, kind: DescramblerDiagnosticKind) -> Self {
        Self::PacketPolicy {
            demux_id,
            pid,
            kind,
        }
    }

    pub fn packet_policy_without_pid(demux_id: i32, kind: DescramblerDiagnosticKind) -> Self {
        Self::PacketPolicyWithoutPid { demux_id, kind }
    }

    pub fn packet_source_filter_validation(
        demux_id: i32,
        pid: PacketPid,
        filter_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    ) -> Self {
        Self::PacketSourceFilterValidation {
            demux_id,
            pid,
            filter_id,
            kind,
            error,
        }
    }

    pub fn cleanup_release_failed(descrambler_id: i32, error: HalError) -> Self {
        Self::CleanupKeyReleaseFailed {
            descrambler_id,
            error,
        }
    }

    pub const fn kind(&self) -> DescramblerDiagnosticKind {
        match self {
            Self::SetKeyTokenFailure { kind, .. } => *kind,
            Self::PidClaimRejected { .. }
            | Self::PidClaimRejectedWithoutDemux { .. }
            | Self::PidClaimRejectedInvalidPid { .. }
            | Self::PidClaimRejectedInvalidPidWithoutDemux { .. } => {
                DescramblerDiagnosticKind::PidClaimRejected
            }
            Self::PacketPolicy { kind, .. } | Self::PacketPolicyWithoutPid { kind, .. } => *kind,
            Self::PacketSourceFilterValidation { kind, .. } => *kind,
            Self::CleanupKeyReleaseFailed { .. } => {
                DescramblerDiagnosticKind::CleanupKeyReleaseFailed
            }
        }
    }

    pub const fn phase(&self) -> DescramblerDiagnosticPhase {
        match self {
            Self::SetKeyTokenFailure { .. } => DescramblerDiagnosticPhase::SetKeyToken,
            Self::PidClaimRejected { phase, .. }
            | Self::PidClaimRejectedWithoutDemux { phase, .. }
            | Self::PidClaimRejectedInvalidPid { phase, .. }
            | Self::PidClaimRejectedInvalidPidWithoutDemux { phase, .. } => *phase,
            Self::PacketPolicy { .. }
            | Self::PacketPolicyWithoutPid { .. }
            | Self::PacketSourceFilterValidation { .. } => {
                DescramblerDiagnosticPhase::PacketPipeline
            }
            Self::CleanupKeyReleaseFailed { .. } => DescramblerDiagnosticPhase::Cleanup,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackArtifactRuntimeSplitPhase {
    OwnerCleanupFinish,
    RegistrationRollbackFinish,
    ObjectCloseCleanupFinish,
    ServiceBootResetFinish,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackArtifactRuntimeSplitOutcome {
    ArtifactFailure {
        artifact_error: HalError,
    },
    RuntimeFinishFailure {
        runtime_error: HalError,
    },
    ArtifactAndRuntimeFailure {
        artifact_error: HalError,
        runtime_error: HalError,
    },
    RuntimeRegistryMissing,
    ServiceBootCallbackArtifactFailure {
        error: HalError,
    },
    ServiceBootDropLeakFailure {
        error: HalError,
    },
    ServiceBootRuntimeFailure {
        error: HalError,
    },
}

impl CallbackArtifactRuntimeSplitOutcome {
    pub fn from_results(
        artifact_error: Option<HalError>,
        runtime_error: Option<HalError>,
    ) -> Option<Self> {
        match (artifact_error, runtime_error) {
            (None, None) => None,
            (Some(artifact_error), None) => Some(Self::ArtifactFailure { artifact_error }),
            (None, Some(runtime_error)) => Some(Self::RuntimeFinishFailure { runtime_error }),
            (Some(artifact_error), Some(runtime_error)) => Some(Self::ArtifactAndRuntimeFailure {
                artifact_error,
                runtime_error,
            }),
        }
    }

    pub fn service_boot_reset_from_attempt_results(
        callback_artifact_result: Result<(), HalError>,
        drop_leak_result: Result<(), HalError>,
        runtime_finish_result: Result<(), HalError>,
    ) -> Vec<Self> {
        let mut outcomes = Vec::new();
        if let Err(error) = callback_artifact_result {
            outcomes.push(Self::ServiceBootCallbackArtifactFailure { error });
        }
        if let Err(error) = drop_leak_result {
            outcomes.push(Self::ServiceBootDropLeakFailure { error });
        }
        if let Err(error) = runtime_finish_result {
            outcomes.push(Self::ServiceBootRuntimeFailure { error });
        }
        outcomes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackArtifactRuntimeSplitTarget {
    Owner {
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    ServiceBootReset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallbackArtifactRuntimeSplitDiagnosticRecord {
    pub phase: CallbackArtifactRuntimeSplitPhase,
    pub target: CallbackArtifactRuntimeSplitTarget,
    pub outcome: CallbackArtifactRuntimeSplitOutcome,
}

impl CallbackArtifactRuntimeSplitDiagnosticRecord {
    pub fn owner(
        phase: CallbackArtifactRuntimeSplitPhase,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        generation: AidlObjectGeneration,
        outcome: CallbackArtifactRuntimeSplitOutcome,
    ) -> Self {
        Self {
            phase,
            target: CallbackArtifactRuntimeSplitTarget::Owner {
                owner_kind,
                owner_id,
                generation,
            },
            outcome,
        }
    }

    pub fn service_boot_reset(outcome: CallbackArtifactRuntimeSplitOutcome) -> Self {
        Self {
            phase: CallbackArtifactRuntimeSplitPhase::ServiceBootResetFinish,
            target: CallbackArtifactRuntimeSplitTarget::ServiceBootReset,
            outcome,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterCallbackDeliveryDiagnosticPhase {
    EventDelivery,
    CallbackRegistryAccounting,
    RuntimeCallbackAccounting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterCallbackDeliveryDiagnosticRecord {
    pub phase: FilterCallbackDeliveryDiagnosticPhase,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub error: HalError,
}

impl FilterCallbackDeliveryDiagnosticRecord {
    pub fn new(
        phase: FilterCallbackDeliveryDiagnosticPhase,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        error: HalError,
    ) -> Self {
        Self {
            phase,
            object_id,
            generation,
            error,
        }
    }
}
