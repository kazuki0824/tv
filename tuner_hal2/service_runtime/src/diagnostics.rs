use std::path::PathBuf;

use maleicacid_tuner_hal2_common::{FrontendBackendKind, HalError};
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
pub struct DescramblerDiagnosticRecord {
    pub kind: DescramblerDiagnosticKind,
    pub phase: DescramblerDiagnosticPhase,
    pub descrambler_id: Option<i32>,
    pub demux_id: Option<i32>,
    pub pid: Option<u16>,
    pub filter_id: Option<i32>,
    pub error: Option<HalError>,
}

impl DescramblerDiagnosticRecord {
    pub fn set_key_token(
        descrambler_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    ) -> Self {
        Self {
            kind,
            phase: DescramblerDiagnosticPhase::SetKeyToken,
            descrambler_id: Some(descrambler_id),
            demux_id: None,
            pid: None,
            filter_id: None,
            error: Some(error),
        }
    }

    pub fn pid_claim(
        phase: DescramblerDiagnosticPhase,
        descrambler_id: i32,
        demux_id: Option<i32>,
        pid: u16,
        filter_id: i32,
        error: HalError,
    ) -> Self {
        Self {
            kind: DescramblerDiagnosticKind::PidClaimRejected,
            phase,
            descrambler_id: Some(descrambler_id),
            demux_id,
            pid: Some(pid),
            filter_id: Some(filter_id),
            error: Some(error),
        }
    }

    pub fn packet_policy(demux_id: i32, pid: u16, kind: DescramblerDiagnosticKind) -> Self {
        Self {
            kind,
            phase: DescramblerDiagnosticPhase::PacketPipeline,
            descrambler_id: None,
            demux_id: Some(demux_id),
            pid: Some(pid),
            filter_id: None,
            error: None,
        }
    }

    pub fn packet_source_filter_validation(
        demux_id: i32,
        pid: u16,
        filter_id: i32,
        kind: DescramblerDiagnosticKind,
        error: HalError,
    ) -> Self {
        Self {
            kind,
            phase: DescramblerDiagnosticPhase::PacketPipeline,
            descrambler_id: None,
            demux_id: Some(demux_id),
            pid: Some(pid),
            filter_id: Some(filter_id),
            error: Some(error),
        }
    }

    pub fn cleanup_release_failed(descrambler_id: i32, error: HalError) -> Self {
        Self {
            kind: DescramblerDiagnosticKind::CleanupKeyReleaseFailed,
            phase: DescramblerDiagnosticPhase::Cleanup,
            descrambler_id: Some(descrambler_id),
            demux_id: None,
            pid: None,
            filter_id: None,
            error: Some(error),
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
