use maleicacid_tuner_hal2_common::HalError;

use crate::AidlApi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunerStatusCode {
    Ok,
    InvalidArgument,
    InvalidState,
    Unavailable,
    OutOfMemory,
    UnknownError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainResult<T> {
    pub status: TunerStatusCode,
    pub value: Option<T>,
    pub error: Option<HalError>,
}

impl<T> DomainResult<T> {
    pub fn ok(value: T) -> Self {
        Self {
            status: TunerStatusCode::Ok,
            value: Some(value),
            error: None,
        }
    }
    pub fn err(error: HalError) -> Self {
        Self {
            status: AidlStatusMapper::map_error(&error),
            value: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AidlFailureSource {
    ObjectLifetime(HalError),
    ProfileUnsupported(HalError),
    InputValidation(HalError),
    RuntimeDispatch(HalError),
    RuntimeFailure(HalError),
    RollbackFailure(HalError),
}

impl AidlFailureSource {
    pub fn step(&self) -> StatusPrecedenceStep {
        match self {
            AidlFailureSource::ObjectLifetime(_) => StatusPrecedenceStep::ObjectLifetime,
            AidlFailureSource::ProfileUnsupported(_) => StatusPrecedenceStep::ProfileUnsupported,
            AidlFailureSource::InputValidation(_) => StatusPrecedenceStep::InputValidation,
            AidlFailureSource::RuntimeDispatch(_) => StatusPrecedenceStep::RuntimeDispatch,
            AidlFailureSource::RuntimeFailure(_) => StatusPrecedenceStep::RuntimeFailure,
            AidlFailureSource::RollbackFailure(_) => StatusPrecedenceStep::RollbackFailure,
        }
    }

    pub fn error(&self) -> &HalError {
        match self {
            AidlFailureSource::ObjectLifetime(error)
            | AidlFailureSource::ProfileUnsupported(error)
            | AidlFailureSource::InputValidation(error)
            | AidlFailureSource::RuntimeDispatch(error)
            | AidlFailureSource::RuntimeFailure(error)
            | AidlFailureSource::RollbackFailure(error) => error,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusPrecedenceStep {
    ObjectLifetime,
    ProfileUnsupported,
    InputValidation,
    RuntimeDispatch,
    RuntimeFailure,
    RollbackFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiStatusPrecedence {
    pub api: AidlApi,
    pub steps: &'static [StatusPrecedenceStep],
}

const DEFAULT_MUTATING_STEPS: &[StatusPrecedenceStep] = &[
    StatusPrecedenceStep::ObjectLifetime,
    StatusPrecedenceStep::InputValidation,
    StatusPrecedenceStep::RuntimeDispatch,
    StatusPrecedenceStep::RuntimeFailure,
    StatusPrecedenceStep::RollbackFailure,
];

const UNSUPPORTED_BEFORE_INPUT_STEPS: &[StatusPrecedenceStep] = &[
    StatusPrecedenceStep::ObjectLifetime,
    StatusPrecedenceStep::ProfileUnsupported,
    StatusPrecedenceStep::InputValidation,
    StatusPrecedenceStep::RuntimeDispatch,
    StatusPrecedenceStep::RuntimeFailure,
    StatusPrecedenceStep::RollbackFailure,
];

pub struct AidlStatusMapper {
    aidl_version: u32,
}

impl AidlStatusMapper {
    pub const fn new(aidl_version: u32) -> Self {
        Self { aidl_version }
    }
    pub const fn aidl_version(&self) -> u32 {
        self.aidl_version
    }

    pub fn map_error(error: &HalError) -> TunerStatusCode {
        match error {
            HalError::ComposedFailure { primary, .. } => Self::map_error(primary),
            HalError::InvalidArgument { .. } => TunerStatusCode::InvalidArgument,
            HalError::InvalidState { .. } => TunerStatusCode::InvalidState,
            HalError::Unsupported(_) | HalError::UnsupportedDetail { .. } => {
                TunerStatusCode::Unavailable
            }
            HalError::DeviceMissing(_)
            | HalError::OpenFailed { .. }
            | HalError::PermissionDenied { .. }
            | HalError::Busy { .. } => TunerStatusCode::Unavailable,
            HalError::OutOfMemory { .. } => TunerStatusCode::OutOfMemory,
            HalError::Internal { .. }
            | HalError::Io { .. }
            | HalError::IoctlFailed { .. }
            | HalError::CallbackFailed { .. }
            | HalError::FmqFailed { .. }
            | HalError::EventFlagFailed { .. }
            | HalError::CleanupFailed { .. } => TunerStatusCode::UnknownError,
        }
    }

    pub const fn precedence_for_api(api: AidlApi) -> ApiStatusPrecedence {
        ApiStatusPrecedence {
            api,
            steps: DEFAULT_MUTATING_STEPS,
        }
    }

    pub const fn unsupported_precedence_for_profile_api(api: AidlApi) -> ApiStatusPrecedence {
        ApiStatusPrecedence {
            api,
            steps: UNSUPPORTED_BEFORE_INPUT_STEPS,
        }
    }

    pub fn resolve_failure_source_by_precedence(
        api: AidlApi,
        failures: &[AidlFailureSource],
        profile_unsupported_precedence: bool,
    ) -> Option<&AidlFailureSource> {
        let precedence = if profile_unsupported_precedence {
            Self::unsupported_precedence_for_profile_api(api)
        } else {
            Self::precedence_for_api(api)
        };
        for step in precedence.steps {
            if let Some(failure) = failures.iter().find(|failure| failure.step() == *step) {
                return Some(failure);
            }
        }
        None
    }

    pub fn resolve_failure_by_precedence(
        api: AidlApi,
        failures: &[AidlFailureSource],
        profile_unsupported_precedence: bool,
    ) -> Option<TunerStatusCode> {
        Self::resolve_failure_source_by_precedence(api, failures, profile_unsupported_precedence)
            .map(|failure| Self::map_error(failure.error()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AIDL_TRANSACTION_TABLE;
    use maleicacid_tuner_hal2_common::{HalInvalidArgumentKind, HalInvalidStateKind};

    #[test]
    fn status_mapper_uses_error_kind_not_display_text() {
        let invalid_arg =
            HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "値域外");
        let invalid_state =
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "閉鎖済み");
        assert_eq!(
            AidlStatusMapper::map_error(&invalid_arg),
            TunerStatusCode::InvalidArgument
        );
        assert_eq!(
            AidlStatusMapper::map_error(&invalid_state),
            TunerStatusCode::InvalidState
        );
    }

    #[test]
    fn device_absent_and_open_failed_are_unavailable_but_runtime_io_is_unknown() {
        use maleicacid_tuner_hal2_common::HalErrorDetail;
        use std::path::PathBuf;
        let missing = HalError::DeviceMissing(PathBuf::from("/dev/missing"));
        let open = HalError::OpenFailed {
            path: PathBuf::from("/dev/dvb/adapter0/frontend0"),
            detail: HalErrorDetail::new("open failed"),
        };
        let ioctl = HalError::IoctlFailed {
            backend: "dvb",
            path: None,
            op: "FE_SET_PROPERTY",
            errno: 5,
        };
        assert_eq!(
            AidlStatusMapper::map_error(&missing),
            TunerStatusCode::Unavailable
        );
        assert_eq!(
            AidlStatusMapper::map_error(&open),
            TunerStatusCode::Unavailable
        );
        assert_eq!(
            AidlStatusMapper::map_error(&ioctl),
            TunerStatusCode::UnknownError
        );
    }

    #[test]
    fn typed_unsupported_dvr_operation_maps_to_unavailable() {
        assert_eq!(
            AidlStatusMapper::map_error(&HalError::Unsupported(
                "DVR operation is unavailable for this DVR kind",
            )),
            TunerStatusCode::Unavailable
        );
    }

    #[test]
    fn callback_fmq_eventflag_and_cleanup_failures_remain_unknown_not_unsupported() {
        let callback = HalError::callback_failed("IFilterCallback", "binder failure");
        let fmq = HalError::fmq_failed("write", "native failure");
        let event = HalError::event_flag_failed("wake", "native failure");
        let cleanup = HalError::cleanup_failed("filter", "queue clear failed");
        assert_eq!(
            AidlStatusMapper::map_error(&callback),
            TunerStatusCode::UnknownError
        );
        assert_eq!(
            AidlStatusMapper::map_error(&fmq),
            TunerStatusCode::UnknownError
        );
        assert_eq!(
            AidlStatusMapper::map_error(&event),
            TunerStatusCode::UnknownError
        );
        assert_eq!(
            AidlStatusMapper::map_error(&cleanup),
            TunerStatusCode::UnknownError
        );
    }

    #[test]
    fn precedence_table_has_entry_for_every_transaction_api() {
        for plan in AIDL_TRANSACTION_TABLE {
            let precedence = AidlStatusMapper::precedence_for_api(plan.api());
            assert_eq!(precedence.api, plan.api());
            assert!(precedence
                .steps
                .contains(&StatusPrecedenceStep::ObjectLifetime));
            assert!(precedence
                .steps
                .contains(&StatusPrecedenceStep::RuntimeDispatch));
        }
    }

    #[test]
    fn profile_unsupported_can_be_declared_before_input_validation_for_ts_only_apis() {
        let precedence =
            AidlStatusMapper::unsupported_precedence_for_profile_api(AidlApi::FilterConfigure);
        assert_eq!(precedence.steps[0], StatusPrecedenceStep::ObjectLifetime);
        assert_eq!(
            precedence.steps[1],
            StatusPrecedenceStep::ProfileUnsupported
        );
        assert_eq!(precedence.steps[2], StatusPrecedenceStep::InputValidation);
    }

    #[test]
    fn resolver_prefers_object_lifetime_before_other_failures() {
        let closed = HalError::invalid_state(
            maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
            "closed object",
        );
        let unsupported = HalError::Unsupported("outside profile");
        let failures = [
            AidlFailureSource::ProfileUnsupported(unsupported),
            AidlFailureSource::ObjectLifetime(closed),
        ];
        assert_eq!(
            AidlStatusMapper::resolve_failure_by_precedence(
                AidlApi::FilterConfigure,
                &failures,
                true
            ),
            Some(TunerStatusCode::InvalidState)
        );
    }

    #[test]
    fn resolver_prefers_profile_unsupported_before_invalid_argument_when_declared() {
        let unsupported =
            HalError::Unsupported("DemuxFilterSettings::ip is outside TS-only profile");
        let invalid =
            HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "invalid ip cid");
        let failures = [
            AidlFailureSource::InputValidation(invalid),
            AidlFailureSource::ProfileUnsupported(unsupported),
        ];
        assert_eq!(
            AidlStatusMapper::resolve_failure_by_precedence(
                AidlApi::FilterConfigure,
                &failures,
                true
            ),
            Some(TunerStatusCode::Unavailable)
        );
    }
}
