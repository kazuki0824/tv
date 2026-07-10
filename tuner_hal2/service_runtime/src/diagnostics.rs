use std::path::PathBuf;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};

use maleicacid_tuner_hal2_common::{FrontendBackendKind, HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::{
    DvrConfigureReport, FilterConfigureReport, PacketPid, QueueRuntimeError, SourceBoundaryReport,
};
use maleicacid_tuner_hal2_descrambler::DescramblerPid;
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub const DEFAULT_DIAGNOSTIC_STORE_LIMIT: usize = 128;

fn saturating_increment_atomic_u64(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

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

    pub fn clear_records_preserving_dropped_count(&mut self) {
        self.records.clear();
    }
}

impl<T> Default for BoundedDiagnosticStore<T> {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_STORE_LIMIT)
    }
}


#[derive(Clone, Debug)]
pub struct DiagnosticSnapshot<TRecord> {
    records: Vec<TRecord>,
    dropped_count: u64,
}

impl<TRecord> DiagnosticSnapshot<TRecord> {
    pub fn new(records: Vec<TRecord>, dropped_count: u64) -> Self {
        Self { records, dropped_count }
    }

    pub fn records(&self) -> &[TRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticKind {
    DeviceMissing,
    DeviceOpenFailed,
    CapabilitySuppressed,
    DuplicateFrontendId,
    DuplicateLnbId,
    CallbackArtifactRuntimeSplitDiagnosticClearFailed,
    DvrPostCommitNotificationDiagnosticClearFailed,
    DvrPostCommitNotificationDiagnosticRecordFailed,
    DvrStatusNotifierCleanupDiagnosticClearFailed,
    DvrStatusNotifierCleanupDiagnosticRecordFailed,
    ObjectCleanupDiagnosticClearFailed,
    FrontendWorkerCleanupDiagnosticClearFailed,
    RuntimeDispatchMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticPhase {
    ProbeDevice,
    OpenDevice,
    CapabilityFilter,
    RegistryCommit,
    DiagnosticReset,
    DispatchValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilitySuppressionReason {
    UnsupportedDeliverySystem,
    DeviceFamilyDisabled,
    NoExportableFrontend,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticRecord {
    DeviceMissing {
        backend: FrontendBackendKind,
        path: PathBuf,
        error: HalError,
    },
    DeviceOpenFailed {
        backend: FrontendBackendKind,
        path: PathBuf,
        error: HalError,
    },
    CapabilitySuppressed {
        backend: FrontendBackendKind,
        path: PathBuf,
        reason: CapabilitySuppressionReason,
    },
    DuplicateFrontendId {
        backend: FrontendBackendKind,
        path: PathBuf,
    },
    DuplicateLnbId {
        backend: FrontendBackendKind,
        path: PathBuf,
    },
    CallbackArtifactRuntimeSplitDiagnosticClearFailed {
        error: HalError,
    },
    DvrPostCommitNotificationDiagnosticClearFailed {
        error: HalError,
    },
    DvrPostCommitNotificationDiagnosticRecordFailed {
        error: HalError,
    },
    DvrStatusNotifierCleanupDiagnosticClearFailed {
        error: HalError,
    },
    DvrStatusNotifierCleanupDiagnosticRecordFailed {
        error: HalError,
    },
    ObjectCleanupDiagnosticClearFailed {
        error: HalError,
    },
    FrontendWorkerCleanupDiagnosticClearFailed {
        error: HalError,
    },
    RuntimeDispatchMissing,
}

impl StartupDiagnosticRecord {
    pub fn device_missing(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self::DeviceMissing {
            backend,
            error: HalError::DeviceMissing(path.clone()),
            path,
        }
    }

    pub fn device_open_failed(
        backend: FrontendBackendKind,
        path: impl Into<PathBuf>,
        error: HalError,
    ) -> Self {
        Self::DeviceOpenFailed {
            backend,
            path: path.into(),
            error,
        }
    }

    pub fn capability_suppressed(
        backend: FrontendBackendKind,
        path: impl Into<PathBuf>,
        reason: CapabilitySuppressionReason,
    ) -> Self {
        Self::CapabilitySuppressed {
            backend,
            path: path.into(),
            reason,
        }
    }

    pub fn duplicate_frontend_id(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        Self::DuplicateFrontendId {
            backend,
            path: path.into(),
        }
    }

    pub fn duplicate_lnb_id(backend: FrontendBackendKind, path: impl Into<PathBuf>) -> Self {
        Self::DuplicateLnbId {
            backend,
            path: path.into(),
        }
    }

    pub fn callback_artifact_runtime_split_diagnostic_clear_failed(error: HalError) -> Self {
        Self::CallbackArtifactRuntimeSplitDiagnosticClearFailed { error }
    }

    pub fn dvr_post_commit_notification_diagnostic_clear_failed(error: HalError) -> Self {
        Self::DvrPostCommitNotificationDiagnosticClearFailed { error }
    }

    pub fn dvr_post_commit_notification_diagnostic_record_failed(error: HalError) -> Self {
        Self::DvrPostCommitNotificationDiagnosticRecordFailed { error }
    }

    pub fn dvr_status_notifier_cleanup_diagnostic_clear_failed(error: HalError) -> Self {
        Self::DvrStatusNotifierCleanupDiagnosticClearFailed { error }
    }

    pub fn dvr_status_notifier_cleanup_diagnostic_record_failed(error: HalError) -> Self {
        Self::DvrStatusNotifierCleanupDiagnosticRecordFailed { error }
    }

    pub fn object_cleanup_diagnostic_clear_failed(error: HalError) -> Self {
        Self::ObjectCleanupDiagnosticClearFailed { error }
    }

    pub fn frontend_worker_cleanup_diagnostic_clear_failed(error: HalError) -> Self {
        Self::FrontendWorkerCleanupDiagnosticClearFailed { error }
    }

    pub fn runtime_dispatch_missing() -> Self {
        Self::RuntimeDispatchMissing
    }

    pub const fn kind(&self) -> StartupDiagnosticKind {
        match self {
            Self::DeviceMissing { .. } => StartupDiagnosticKind::DeviceMissing,
            Self::DeviceOpenFailed { .. } => StartupDiagnosticKind::DeviceOpenFailed,
            Self::CapabilitySuppressed { .. } => StartupDiagnosticKind::CapabilitySuppressed,
            Self::DuplicateFrontendId { .. } => StartupDiagnosticKind::DuplicateFrontendId,
            Self::DuplicateLnbId { .. } => StartupDiagnosticKind::DuplicateLnbId,
            Self::CallbackArtifactRuntimeSplitDiagnosticClearFailed { .. } => {
                StartupDiagnosticKind::CallbackArtifactRuntimeSplitDiagnosticClearFailed
            }
            Self::DvrPostCommitNotificationDiagnosticClearFailed { .. } => {
                StartupDiagnosticKind::DvrPostCommitNotificationDiagnosticClearFailed
            }
            Self::DvrPostCommitNotificationDiagnosticRecordFailed { .. } => {
                StartupDiagnosticKind::DvrPostCommitNotificationDiagnosticRecordFailed
            }
            Self::DvrStatusNotifierCleanupDiagnosticClearFailed { .. } => {
                StartupDiagnosticKind::DvrStatusNotifierCleanupDiagnosticClearFailed
            }
            Self::DvrStatusNotifierCleanupDiagnosticRecordFailed { .. } => {
                StartupDiagnosticKind::DvrStatusNotifierCleanupDiagnosticRecordFailed
            }
            Self::ObjectCleanupDiagnosticClearFailed { .. } => {
                StartupDiagnosticKind::ObjectCleanupDiagnosticClearFailed
            }
            Self::FrontendWorkerCleanupDiagnosticClearFailed { .. } => {
                StartupDiagnosticKind::FrontendWorkerCleanupDiagnosticClearFailed
            }
            Self::RuntimeDispatchMissing => StartupDiagnosticKind::RuntimeDispatchMissing,
        }
    }

    pub const fn phase(&self) -> StartupDiagnosticPhase {
        match self {
            Self::DeviceMissing { .. } => StartupDiagnosticPhase::ProbeDevice,
            Self::DeviceOpenFailed { .. } => StartupDiagnosticPhase::OpenDevice,
            Self::CapabilitySuppressed { .. } => StartupDiagnosticPhase::CapabilityFilter,
            Self::DuplicateFrontendId { .. } | Self::DuplicateLnbId { .. } => {
                StartupDiagnosticPhase::RegistryCommit
            }
            Self::CallbackArtifactRuntimeSplitDiagnosticClearFailed { .. }
            | Self::DvrPostCommitNotificationDiagnosticClearFailed { .. }
            | Self::DvrPostCommitNotificationDiagnosticRecordFailed { .. }
            | Self::DvrStatusNotifierCleanupDiagnosticClearFailed { .. }
            | Self::DvrStatusNotifierCleanupDiagnosticRecordFailed { .. }
            | Self::ObjectCleanupDiagnosticClearFailed { .. }
            | Self::FrontendWorkerCleanupDiagnosticClearFailed { .. } => {
                StartupDiagnosticPhase::DiagnosticReset
            }
            Self::RuntimeDispatchMissing => StartupDiagnosticPhase::DispatchValidation,
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
pub enum ChildOpenRollbackOutcome {
    ObjectRegistrationRollbackFailed {
        object_error: HalError,
    },
    RuntimeCleanupMissing {
        runtime_cleanup_error: HalError,
    },
    BothFailed {
        object_error: HalError,
        runtime_cleanup_error: HalError,
    },
}

impl ChildOpenRollbackOutcome {
    pub const fn kind(&self) -> ChildOpenRollbackKind {
        match self {
            Self::ObjectRegistrationRollbackFailed { .. } => {
                ChildOpenRollbackKind::ObjectRegistrationRollbackFailed
            }
            Self::RuntimeCleanupMissing { .. } => ChildOpenRollbackKind::RuntimeCleanupMissing,
            Self::BothFailed { .. } => ChildOpenRollbackKind::BothFailed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildOpenRollbackDiagnosticRecord {
    pub phase: ChildOpenRollbackPhase,
    pub object_kind: AidlObjectKind,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub runtime_id: i32,
    pub outcome: ChildOpenRollbackOutcome,
}

impl ChildOpenRollbackDiagnosticRecord {
    pub fn new(
        phase: ChildOpenRollbackPhase,
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        runtime_id: i32,
        outcome: ChildOpenRollbackOutcome,
    ) -> Self {
        Self {
            phase,
            object_kind,
            object_id,
            generation,
            runtime_id,
            outcome,
        }
    }

    pub const fn kind(&self) -> ChildOpenRollbackKind {
        self.outcome.kind()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrPostCommitNotificationPhase {
    InitialStatusDelivery,
    StatusNotifierStart,
    StatusNotifierStop,
    StatusNotifierRuntimeFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrPostCommitNotificationFailureKind {
    CallbackArtifactLookup,
    RuntimePolicySkip,
    EventConversion,
    BinderDelivery,
    PostCommitNotification,
    NotifierTerminal,
    NotifierCleanup,
    NotifierPreflight,
    CallbackRegistryAccounting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrPostCommitNotificationDiagnosticRecord {
    pub phase: DvrPostCommitNotificationPhase,
    pub failure_kind: DvrPostCommitNotificationFailureKind,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub error: HalError,
}

#[derive(Clone, Debug)]
pub struct DvrPostCommitNotificationDiagnosticSnapshot {
    records: Vec<DvrPostCommitNotificationDiagnosticRecord>,
    dropped_count: u64,
    record_failure_count: u64,
}

impl DvrPostCommitNotificationDiagnosticSnapshot {
    pub fn records(&self) -> &[DvrPostCommitNotificationDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn record_failure_count(&self) -> u64 {
        self.record_failure_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrStatusNotifierCleanupDiagnosticKind {
    ResetStoreRecoveredAfterPoison,
    ResetNotifierCleanup,
    WorkerTerminal,
    SupersedeCleanup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrStatusNotifierCleanupDiagnosticRecord {
    pub kind: DvrStatusNotifierCleanupDiagnosticKind,
    pub phase: DvrPostCommitNotificationPhase,
    pub object_id: Option<AidlObjectId>,
    pub generation: Option<AidlObjectGeneration>,
    pub result: Result<(), HalError>,
}

impl DvrStatusNotifierCleanupDiagnosticRecord {
    pub fn reset_store_recovered_after_poison(error: HalError) -> Self {
        Self {
            kind: DvrStatusNotifierCleanupDiagnosticKind::ResetStoreRecoveredAfterPoison,
            phase: DvrPostCommitNotificationPhase::StatusNotifierStop,
            object_id: None,
            generation: None,
            result: Err(error),
        }
    }

    pub fn reset_notifier_cleanup(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        result: Result<(), HalError>,
    ) -> Self {
        Self {
            kind: DvrStatusNotifierCleanupDiagnosticKind::ResetNotifierCleanup,
            phase: DvrPostCommitNotificationPhase::StatusNotifierStop,
            object_id: Some(object_id),
            generation: Some(generation),
            result,
        }
    }

    pub fn worker_terminal(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        result: Result<(), HalError>,
    ) -> Self {
        Self {
            kind: DvrStatusNotifierCleanupDiagnosticKind::WorkerTerminal,
            phase: DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
            object_id: Some(object_id),
            generation: Some(generation),
            result,
        }
    }

    pub fn supersede_cleanup(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        result: Result<(), HalError>,
    ) -> Self {
        Self {
            kind: DvrStatusNotifierCleanupDiagnosticKind::SupersedeCleanup,
            phase: DvrPostCommitNotificationPhase::StatusNotifierStop,
            object_id: Some(object_id),
            generation: Some(generation),
            result,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DvrStatusNotifierCleanupDiagnosticSnapshot {
    records: Vec<DvrStatusNotifierCleanupDiagnosticRecord>,
    dropped_count: u64,
    record_failure_count: u64,
}

impl DvrStatusNotifierCleanupDiagnosticSnapshot {
    pub fn records(&self) -> &[DvrStatusNotifierCleanupDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn record_failure_count(&self) -> u64 {
        self.record_failure_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueDescriptorQueryDiagnosticRecord {
    pub object_kind: AidlObjectKind,
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub runtime_id: i32,
    pub error: QueueRuntimeError,
}

impl QueueDescriptorQueryDiagnosticRecord {
    pub const fn new(
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        runtime_id: i32,
        error: QueueRuntimeError,
    ) -> Self {
        Self {
            object_kind,
            object_id,
            generation,
            runtime_id,
            error,
        }
    }
}

impl DvrPostCommitNotificationDiagnosticRecord {
    pub fn new(
        phase: DvrPostCommitNotificationPhase,
        failure_kind: DvrPostCommitNotificationFailureKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        error: HalError,
    ) -> Self {
        Self {
            phase,
            failure_kind,
            object_id,
            generation,
            error,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SharedDvrPostCommitNotificationDiagnostics {
    records: Arc<Mutex<BoundedDiagnosticStore<DvrPostCommitNotificationDiagnosticRecord>>>,
    record_failure_count: Arc<AtomicU64>,
}

impl SharedDvrPostCommitNotificationDiagnostics {
    pub fn new(limit: usize) -> Self {
        Self {
            records: Arc::new(Mutex::new(BoundedDiagnosticStore::new(limit))),
            record_failure_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record(
        &self,
        record: DvrPostCommitNotificationDiagnosticRecord,
    ) -> Result<(), HalError> {
        match self.records.lock() {
            Ok(mut records) => {
                records.push(record);
                Ok(())
            }
            Err(_) => {
                saturating_increment_atomic_u64(&self.record_failure_count);
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR post-commit notification diagnostic store lock poisoned",
                ))
            }
        }
    }

    pub fn snapshot(&self) -> Result<DvrPostCommitNotificationDiagnosticSnapshot, HalError> {
        let records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR post-commit notification diagnostic store lock poisoned while snapshotting",
            )
        })?;
        Ok(DvrPostCommitNotificationDiagnosticSnapshot {
            records: records.as_slice().to_vec(),
            dropped_count: records.dropped_count(),
            record_failure_count: self.record_failure_count.load(Ordering::Relaxed),
        })
    }

    pub fn clear(&self) -> Result<(), HalError> {
        let mut records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR post-commit notification diagnostic store lock poisoned while clearing",
            )
        })?;
        records.clear();
        self.record_failure_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}

impl Default for SharedDvrPostCommitNotificationDiagnostics {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_STORE_LIMIT)
    }
}

#[derive(Clone, Debug)]
pub struct SharedDvrStatusNotifierCleanupDiagnostics {
    records: Arc<Mutex<BoundedDiagnosticStore<DvrStatusNotifierCleanupDiagnosticRecord>>>,
    record_failure_count: Arc<AtomicU64>,
}

impl SharedDvrStatusNotifierCleanupDiagnostics {
    pub fn new(limit: usize) -> Self {
        Self {
            records: Arc::new(Mutex::new(BoundedDiagnosticStore::new(limit))),
            record_failure_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record(&self, record: DvrStatusNotifierCleanupDiagnosticRecord) -> Result<(), HalError> {
        match self.records.lock() {
            Ok(mut records) => {
                records.push(record);
                Ok(())
            }
            Err(_) => {
                saturating_increment_atomic_u64(&self.record_failure_count);
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR status notifier cleanup diagnostic store lock poisoned",
                ))
            }
        }
    }

    pub fn snapshot(&self) -> Result<DvrStatusNotifierCleanupDiagnosticSnapshot, HalError> {
        let records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier cleanup diagnostic store lock poisoned while snapshotting",
            )
        })?;
        Ok(DvrStatusNotifierCleanupDiagnosticSnapshot {
            records: records.as_slice().to_vec(),
            dropped_count: records.dropped_count(),
            record_failure_count: self.record_failure_count.load(Ordering::Relaxed),
        })
    }

    pub fn clear(&self) -> Result<(), HalError> {
        let mut records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier cleanup diagnostic store lock poisoned while clearing",
            )
        })?;
        records.clear();
        self.record_failure_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}

impl Default for SharedDvrStatusNotifierCleanupDiagnostics {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_STORE_LIMIT)
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


#[derive(Clone, Debug)]
pub struct CallbackArtifactRuntimeSplitDiagnosticSnapshot {
    records: Vec<CallbackArtifactRuntimeSplitDiagnosticRecord>,
    dropped_count: u64,
}

impl CallbackArtifactRuntimeSplitDiagnosticSnapshot {
    pub fn records(&self) -> &[CallbackArtifactRuntimeSplitDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }
}

#[derive(Clone, Debug)]
pub struct SharedCallbackArtifactRuntimeSplitDiagnostics {
    records: Arc<Mutex<BoundedDiagnosticStore<CallbackArtifactRuntimeSplitDiagnosticRecord>>>,
}

impl SharedCallbackArtifactRuntimeSplitDiagnostics {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(BoundedDiagnosticStore::default())),
        }
    }

    pub fn record(
        &self,
        record: CallbackArtifactRuntimeSplitDiagnosticRecord,
    ) -> Result<(), HalError> {
        let mut records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "callback artifact runtime split diagnostic store lock poisoned",
            )
        })?;
        records.push(record);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<CallbackArtifactRuntimeSplitDiagnosticSnapshot, HalError> {
        let records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "callback artifact runtime split diagnostic store lock poisoned while snapshotting",
            )
        })?;
        Ok(CallbackArtifactRuntimeSplitDiagnosticSnapshot {
            records: records.as_slice().to_vec(),
            dropped_count: records.dropped_count(),
        })
    }

    pub fn clear(&self) -> Result<(), HalError> {
        let mut records = self.records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "callback artifact runtime split diagnostic store lock poisoned while clearing",
            )
        })?;
        records.clear();
        Ok(())
    }
}

impl Default for SharedCallbackArtifactRuntimeSplitDiagnostics {
    fn default() -> Self {
        Self::new()
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
    RuntimeFinishAndArtifactCleanupFailure {
        runtime_error: HalError,
        cleanup_error: HalError,
    },
    RuntimeRegistryMissing,
    ServiceBootDvrNotifierFailure {
        error: HalError,
    },
    ServiceBootCallbackArtifactFailure {
        error: HalError,
    },
    ServiceBootDropLeakFailure {
        error: HalError,
    },
    ServiceBootCallbackFallbackDiagnosticClearFailure {
        error: HalError,
    },
    ServiceBootDiagnosticClearFailure {
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

    pub fn runtime_finish_and_artifact_cleanup_failure(
        runtime_error: HalError,
        cleanup_error: HalError,
    ) -> Self {
        Self::RuntimeFinishAndArtifactCleanupFailure {
            runtime_error,
            cleanup_error,
        }
    }

    pub fn service_boot_reset_from_attempt_results(
        dvr_notifier_result: Result<(), HalError>,
        callback_artifact_result: Result<(), HalError>,
        drop_leak_result: Result<(), HalError>,
        callback_fallback_clear_result: Result<(), HalError>,
        diagnostic_clear_result: Result<(), HalError>,
        runtime_finish_result: Result<(), HalError>,
    ) -> Vec<Self> {
        let mut outcomes = Vec::new();
        if let Err(error) = dvr_notifier_result {
            outcomes.push(Self::ServiceBootDvrNotifierFailure { error });
        }
        if let Err(error) = callback_artifact_result {
            outcomes.push(Self::ServiceBootCallbackArtifactFailure { error });
        }
        if let Err(error) = drop_leak_result {
            outcomes.push(Self::ServiceBootDropLeakFailure { error });
        }
        if let Err(error) = callback_fallback_clear_result {
            outcomes.push(Self::ServiceBootCallbackFallbackDiagnosticClearFailure { error });
        }
        if let Err(error) = diagnostic_clear_result {
            outcomes.push(Self::ServiceBootDiagnosticClearFailure { error });
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendCallbackDeliveryDiagnosticPhase {
    CallbackArtifactLookup,
    ScanEndDelivery,
    ScanSessionAccounting,
    CallbackRegistryAccounting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendCallbackDeliveryDiagnosticRecord {
    CallbackArtifactLookup {
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        error: HalError,
    },
    ScanEndDelivery {
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    },
    ScanSessionAccounting {
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    },
    CallbackRegistryAccounting {
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    },
}

impl FrontendCallbackDeliveryDiagnosticRecord {
    pub fn callback_artifact_lookup(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        error: HalError,
    ) -> Self {
        Self::CallbackArtifactLookup {
            object_id,
            generation,
            error,
        }
    }

    pub fn scan_end_delivery(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    ) -> Self {
        Self::ScanEndDelivery {
            object_id,
            generation,
            frontend_id,
            scan_generation,
            error,
        }
    }

    pub fn scan_session_accounting(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    ) -> Self {
        Self::ScanSessionAccounting {
            object_id,
            generation,
            frontend_id,
            scan_generation,
            error,
        }
    }

    pub fn callback_registry_accounting(
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        error: HalError,
    ) -> Self {
        Self::CallbackRegistryAccounting {
            object_id,
            generation,
            frontend_id,
            scan_generation,
            error,
        }
    }

    pub const fn phase(&self) -> FrontendCallbackDeliveryDiagnosticPhase {
        match self {
            Self::CallbackArtifactLookup { .. } => {
                FrontendCallbackDeliveryDiagnosticPhase::CallbackArtifactLookup
            }
            Self::ScanEndDelivery { .. } => FrontendCallbackDeliveryDiagnosticPhase::ScanEndDelivery,
            Self::ScanSessionAccounting { .. } => {
                FrontendCallbackDeliveryDiagnosticPhase::ScanSessionAccounting
            }
            Self::CallbackRegistryAccounting { .. } => {
                FrontendCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting
            }
        }
    }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DemuxTransactionDiagnosticId(pub u64);

impl DemuxTransactionDiagnosticId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxTransactionDiagnosticKind {
    SourceBoundary,
    FilterConfigure,
    DvrConfigure,
    FilterRuntimeOperation,
}

pub type StartupDiagnosticSnapshot = DiagnosticSnapshot<StartupDiagnosticRecord>;
pub type DescramblerDiagnosticSnapshot = DiagnosticSnapshot<DescramblerDiagnosticRecord>;
pub type ChildOpenRollbackDiagnosticSnapshot = DiagnosticSnapshot<ChildOpenRollbackDiagnosticRecord>;
pub type QueueDescriptorQueryDiagnosticSnapshot = DiagnosticSnapshot<QueueDescriptorQueryDiagnosticRecord>;
#[derive(Clone, Debug)]
pub struct FilterCallbackDeliveryDiagnosticSnapshot {
    records: Vec<FilterCallbackDeliveryDiagnosticRecord>,
    dropped_count: u64,
    runtime_snapshot_missing: bool,
    fallback_record_count: usize,
    fallback_dropped_count: u64,
    fallback_record_failure_count: u64,
}

impl FilterCallbackDeliveryDiagnosticSnapshot {
    pub fn new(records: Vec<FilterCallbackDeliveryDiagnosticRecord>, dropped_count: u64) -> Self {
        Self::new_with_metadata(records, dropped_count, false, 0, 0, 0)
    }

    pub fn new_with_metadata(
        records: Vec<FilterCallbackDeliveryDiagnosticRecord>,
        dropped_count: u64,
        runtime_snapshot_missing: bool,
        fallback_record_count: usize,
        fallback_dropped_count: u64,
        fallback_record_failure_count: u64,
    ) -> Self {
        Self {
            records,
            dropped_count,
            runtime_snapshot_missing,
            fallback_record_count,
            fallback_dropped_count,
            fallback_record_failure_count,
        }
    }

    pub fn records(&self) -> &[FilterCallbackDeliveryDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn runtime_snapshot_missing(&self) -> bool {
        self.runtime_snapshot_missing
    }

    pub const fn fallback_record_count(&self) -> usize {
        self.fallback_record_count
    }

    pub const fn fallback_dropped_count(&self) -> u64 {
        self.fallback_dropped_count
    }

    pub const fn fallback_record_failure_count(&self) -> u64 {
        self.fallback_record_failure_count
    }
}

#[derive(Clone, Debug)]
pub struct FrontendCallbackDeliveryDiagnosticSnapshot {
    records: Vec<FrontendCallbackDeliveryDiagnosticRecord>,
    dropped_count: u64,
    runtime_snapshot_missing: bool,
    fallback_record_count: usize,
    fallback_dropped_count: u64,
    fallback_record_failure_count: u64,
}

impl FrontendCallbackDeliveryDiagnosticSnapshot {
    pub fn new(records: Vec<FrontendCallbackDeliveryDiagnosticRecord>, dropped_count: u64) -> Self {
        Self::new_with_metadata(records, dropped_count, false, 0, 0, 0)
    }

    pub fn new_with_metadata(
        records: Vec<FrontendCallbackDeliveryDiagnosticRecord>,
        dropped_count: u64,
        runtime_snapshot_missing: bool,
        fallback_record_count: usize,
        fallback_dropped_count: u64,
        fallback_record_failure_count: u64,
    ) -> Self {
        Self {
            records,
            dropped_count,
            runtime_snapshot_missing,
            fallback_record_count,
            fallback_dropped_count,
            fallback_record_failure_count,
        }
    }

    pub fn records(&self) -> &[FrontendCallbackDeliveryDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub const fn runtime_snapshot_missing(&self) -> bool {
        self.runtime_snapshot_missing
    }

    pub const fn fallback_record_count(&self) -> usize {
        self.fallback_record_count
    }

    pub const fn fallback_dropped_count(&self) -> u64 {
        self.fallback_dropped_count
    }

    pub const fn fallback_record_failure_count(&self) -> u64 {
        self.fallback_record_failure_count
    }
}

#[derive(Clone, Debug)]
pub struct DemuxTransactionDiagnosticSnapshot {
    records: Vec<DemuxTransactionDiagnosticRecord>,
    dropped_count: u64,
}

impl DemuxTransactionDiagnosticSnapshot {
    pub(crate) fn new(records: Vec<DemuxTransactionDiagnosticRecord>, dropped_count: u64) -> Self {
        Self { records, dropped_count }
    }

    pub fn records(&self) -> &[DemuxTransactionDiagnosticRecord] {
        &self.records
    }

    pub const fn dropped_count(&self) -> u64 {
        self.dropped_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DemuxTransactionDiagnosticRecord {
    SourceBoundary {
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: Option<i32>,
        report: SourceBoundaryReport,
        error: HalError,
    },
    FilterConfigure {
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        filter_id: i32,
        report: FilterConfigureReport,
        error: HalError,
    },
    DvrConfigure {
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        dvr_id: i32,
        report: DvrConfigureReport,
        error: HalError,
    },
    FilterRuntimeOperation {
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        filter_id: i32,
        report: maleicacid_tuner_hal2_demux::FilterRuntimeOperationReport,
        error: HalError,
    },
}

impl DemuxTransactionDiagnosticRecord {
    pub fn source_boundary(
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        report: SourceBoundaryReport,
        error: HalError,
    ) -> Self {
        let sink_filter_id = report.sink_filter_id();
        let source_filter_id = report.source_filter_id();
        Self::SourceBoundary {
            diagnostic_id,
            demux_id,
            sink_filter_id,
            source_filter_id,
            report,
            error,
        }
    }

    pub fn filter_configure(
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        filter_id: i32,
        report: FilterConfigureReport,
        error: HalError,
    ) -> Self {
        Self::FilterConfigure {
            diagnostic_id,
            demux_id,
            filter_id,
            report,
            error,
        }
    }

    pub fn dvr_configure(
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        dvr_id: i32,
        report: DvrConfigureReport,
        error: HalError,
    ) -> Self {
        Self::DvrConfigure {
            diagnostic_id,
            demux_id,
            dvr_id,
            report,
            error,
        }
    }

    pub fn filter_runtime_operation(
        diagnostic_id: DemuxTransactionDiagnosticId,
        demux_id: i32,
        filter_id: i32,
        report: maleicacid_tuner_hal2_demux::FilterRuntimeOperationReport,
        error: HalError,
    ) -> Self {
        Self::FilterRuntimeOperation {
            diagnostic_id,
            demux_id,
            filter_id,
            report,
            error,
        }
    }

    pub const fn diagnostic_id(&self) -> DemuxTransactionDiagnosticId {
        match self {
            Self::SourceBoundary { diagnostic_id, .. }
            | Self::FilterConfigure { diagnostic_id, .. }
            | Self::DvrConfigure { diagnostic_id, .. }
            | Self::FilterRuntimeOperation { diagnostic_id, .. } => *diagnostic_id,
        }
    }

    pub const fn kind(&self) -> DemuxTransactionDiagnosticKind {
        match self {
            Self::SourceBoundary { .. } => DemuxTransactionDiagnosticKind::SourceBoundary,
            Self::FilterConfigure { .. } => DemuxTransactionDiagnosticKind::FilterConfigure,
            Self::DvrConfigure { .. } => DemuxTransactionDiagnosticKind::DvrConfigure,
            Self::FilterRuntimeOperation { .. } => {
                DemuxTransactionDiagnosticKind::FilterRuntimeOperation
            }
        }
    }
}
