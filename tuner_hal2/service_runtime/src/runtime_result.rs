use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::RuntimeTransactionName;
use maleicacid_tuner_hal2_resource_ledger::LedgerError;

use crate::dispatch::ServiceRuntimeDispatchTarget;
use crate::object_table::RuntimeObjectTableError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHandlerCoverage {
    Connected,
    NotConnected,
    UnsupportedByDesign,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeHandlerSuccess {
    Planned,
    UnsupportedByDesign,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeHandlerResult {
    pub transaction: RuntimeTransactionName,
    pub target: ServiceRuntimeDispatchTarget,
    pub success: RuntimeHandlerSuccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeHandlerError {
    NotConnected {
        transaction: RuntimeTransactionName,
        target: ServiceRuntimeDispatchTarget,
    },
    GenerationMismatch {
        transaction: RuntimeTransactionName,
        source: RuntimeObjectTableError,
    },
    MissingObject {
        transaction: RuntimeTransactionName,
        source: RuntimeObjectTableError,
    },
    InvalidOwner {
        transaction: RuntimeTransactionName,
        source: RuntimeObjectTableError,
    },
    InputValidation {
        transaction: RuntimeTransactionName,
        source: HalError,
    },
    UnsupportedProfile {
        transaction: RuntimeTransactionName,
    },
    Unsupported {
        transaction: RuntimeTransactionName,
    },
    RuntimeFailure {
        transaction: RuntimeTransactionName,
        source: RuntimeHandlerFailureSource,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeHandlerFailureSource {
    ObjectTable(RuntimeObjectTableError),
    Ledger(LedgerError),
    Internal(HalError),
}

impl RuntimeHandlerError {
    pub fn into_hal_error(self) -> HalError {
        match self {
            Self::NotConnected { .. } => HalError::internal(
                HalInternalKind::InvariantViolation,
                "runtime handler is not connected",
            ),
            Self::GenerationMismatch { .. } => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "runtime object generation mismatch",
            ),
            Self::MissingObject { .. } => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "runtime object is missing",
            ),
            Self::InvalidOwner { .. } => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "runtime object owner mismatch",
            ),
            Self::InputValidation { source, .. } => source,
            Self::UnsupportedProfile { .. } => HalError::Unsupported(
                "AIDL input variant is outside the TS-only tuner_hal2 profile",
            ),
            Self::Unsupported { .. } => {
                HalError::Unsupported("runtime transaction is unsupported by design")
            }
            Self::RuntimeFailure { source, .. } => match source {
                RuntimeHandlerFailureSource::Internal(error) => error,
                RuntimeHandlerFailureSource::ObjectTable(_) => HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "runtime object table failure",
                ),
                RuntimeHandlerFailureSource::Ledger(_) => HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "runtime ledger failure",
                ),
            },
        }
    }
}
