use crate::{RegistryCommitError, RuntimeCommandDispatchError, RuntimeObjectTableError};
use maleicacid_tuner_hal2_common::{
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};

pub(crate) fn command_dispatch_error_to_hal(error: RuntimeCommandDispatchError) -> HalError {
    error.into_hal_error()
}

pub fn object_table_error_to_hal(error: RuntimeObjectTableError) -> HalError {
    match error {
        RuntimeObjectTableError::MissingObject { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is closed or missing",
        ),
        RuntimeObjectTableError::ObjectKindMismatch { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch",
        ),
        RuntimeObjectTableError::GenerationMismatch { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object generation mismatch",
        ),
        RuntimeObjectTableError::InvalidOwner { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object owner mismatch",
        ),
        RuntimeObjectTableError::MissingOwner { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object owner is missing",
        ),
        RuntimeObjectTableError::OwnerGenerationMismatch { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object owner generation mismatch",
        ),
        RuntimeObjectTableError::OwnerKindMismatch { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object owner kind mismatch",
        ),
        RuntimeObjectTableError::OwnerNotLive { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object owner is not live",
        ),
        RuntimeObjectTableError::InvalidLifecycle { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is not live",
        ),
        RuntimeObjectTableError::DuplicateObjectId { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object id already registered",
        ),
        RuntimeObjectTableError::DuplicateRuntimeBinding { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL public runtime object is already opened",
        ),
        RuntimeObjectTableError::UnsupportedObjectKind { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind is unsupported",
        ),
        RuntimeObjectTableError::GenerationOverflow => HalError::internal(
            HalInternalKind::InvariantViolation,
            "AIDL object generation overflow",
        ),
    }
}

pub(crate) fn registry_commit_error_to_hal(
    error: RegistryCommitError,
    context: &'static str,
) -> HalError {
    match error {
        RegistryCommitError::MissingFrontendId { .. }
        | RegistryCommitError::MissingLnbId { .. }
        | RegistryCommitError::LnbFrontendMismatch { .. } => {
            HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, context)
        }
        RegistryCommitError::DuplicateFrontendId { .. }
        | RegistryCommitError::DuplicateDemuxId { .. }
        | RegistryCommitError::DuplicateLnbId { .. }
        | RegistryCommitError::DuplicateFilterId { .. }
        | RegistryCommitError::DuplicateDvrId { .. }
        | RegistryCommitError::DuplicateDescramblerId { .. } => {
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, context)
        }
        RegistryCommitError::RuntimeIdExhausted { .. } => {
            HalError::internal(HalInternalKind::InvariantViolation, context)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DemuxRuntimeId, RuntimeRegistryKind};

    #[test]
    fn object_duplicate_runtime_binding_maps_to_invalid_state() {
        let error = object_table_error_to_hal(RuntimeObjectTableError::DuplicateRuntimeBinding {
            object_kind: maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
            runtime_id: maleicacid_tuner_hal2_resource_ledger::LedgerId(7),
        });

        assert!(matches!(error, HalError::InvalidState { .. }));
    }

    #[test]
    fn registry_duplicate_maps_to_invalid_state() {
        let error = registry_commit_error_to_hal(
            RegistryCommitError::DuplicateDemuxId {
                id: DemuxRuntimeId(3),
            },
            "registry commit failed",
        );

        assert!(matches!(error, HalError::InvalidState { .. }));
    }

    #[test]
    fn registry_runtime_id_exhausted_maps_to_internal() {
        let error = registry_commit_error_to_hal(
            RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Demux,
            },
            "registry id exhausted",
        );

        assert!(matches!(error, HalError::Internal { .. }));
    }
}
