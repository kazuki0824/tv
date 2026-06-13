use std::path::PathBuf;

use maleicacid_tuner_hal2_common::{FrontendBackendKind, HalError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupDiagnosticKind {
    DeviceMissing,
    DeviceOpenFailed,
    CapabilitySuppressed,
    DuplicateFrontendId,
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
