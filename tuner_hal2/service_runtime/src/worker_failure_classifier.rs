use maleicacid_tuner_hal2_common::HalError;

use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
use crate::diagnostics::WorkerFailureCategory;
use crate::worker_runtime::WorkerTerminalResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClassifiedCallbackFailure {
    report: CallbackDeliveryFailureReport,
    category: WorkerFailureCategory,
}

impl ClassifiedCallbackFailure {
    pub(crate) fn into_parts(self) -> (CallbackDeliveryFailureReport, WorkerFailureCategory) {
        (self.report, self.category)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassifiedWorkerTerminalResult<T> {
    Normal(T),
    StopRequested,
    Failure {
        category: WorkerFailureCategory,
        error: HalError,
    },
}

impl<T> ClassifiedWorkerTerminalResult<T> {
    pub fn into_failure(self) -> Option<(WorkerFailureCategory, HalError)> {
        match self {
            Self::Failure { category, error } => Some((category, error)),
            Self::Normal(_) | Self::StopRequested => None,
        }
    }
}

pub struct WorkerFailureClassifier;

impl WorkerFailureClassifier {
    pub(crate) fn classify_terminal<T>(
        result: WorkerTerminalResult<T>,
        panic_context: &'static str,
    ) -> ClassifiedWorkerTerminalResult<T> {
        match result {
            WorkerTerminalResult::Normal(value) => ClassifiedWorkerTerminalResult::Normal(value),
            WorkerTerminalResult::StopRequested => ClassifiedWorkerTerminalResult::StopRequested,
            WorkerTerminalResult::RuntimeFailure(error) => {
                ClassifiedWorkerTerminalResult::Failure {
                    category: Self::classify_runtime_failure(&error),
                    error,
                }
            }
            WorkerTerminalResult::PanicOrJoinFailure => ClassifiedWorkerTerminalResult::Failure {
                category: Self::classify_join_failure(),
                error: HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    panic_context,
                ),
            },
        }
    }
    pub(crate) fn classify_callback(
        report: CallbackDeliveryFailureReport,
    ) -> ClassifiedCallbackFailure {
        let category = match report.phase() {
            CallbackDeliveryFailurePhase::PostDeliveryCommit => {
                WorkerFailureCategory::CallbackCommit
            }
            CallbackDeliveryFailurePhase::CallbackArtifactLookup => {
                WorkerFailureCategory::CallbackArtifact
            }
            CallbackDeliveryFailurePhase::RuntimePolicySkip
            | CallbackDeliveryFailurePhase::NotifierPreflight => {
                WorkerFailureCategory::CallbackPolicy
            }
            CallbackDeliveryFailurePhase::EventConversion
            | CallbackDeliveryFailurePhase::ScanEndDelivery => {
                WorkerFailureCategory::CallbackConversion
            }
            CallbackDeliveryFailurePhase::BinderDelivery
            | CallbackDeliveryFailurePhase::PostCommitNotification => {
                WorkerFailureCategory::CallbackBinder
            }
            CallbackDeliveryFailurePhase::NotifierTerminal => {
                WorkerFailureCategory::CallbackNotifierTerminal
            }
            CallbackDeliveryFailurePhase::NotifierCleanup => WorkerFailureCategory::CallbackCleanup,
        };
        ClassifiedCallbackFailure { report, category }
    }

    pub(crate) const fn classify_join_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::Join
    }

    fn classify_runtime_failure(error: &HalError) -> WorkerFailureCategory {
        match error.primary_error() {
            HalError::IoctlFailed { .. } | HalError::Io { .. } => WorkerFailureCategory::BackendControl,
            HalError::CallbackFailed { .. } => WorkerFailureCategory::CallbackBinder,
            HalError::FmqFailed { .. } => WorkerFailureCategory::Fmq,
            HalError::EventFlagFailed { .. } => WorkerFailureCategory::EventFlag,
            HalError::CleanupFailed { .. } => WorkerFailureCategory::Cleanup,
            HalError::WorkerLockPoisoned { lock: maleicacid_tuner_hal2_common::WorkerLockKind::Wake, .. } => WorkerFailureCategory::Wake,
            HalError::WorkerLockPoisoned { .. } => WorkerFailureCategory::LockPoison,
            _ => WorkerFailureCategory::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{HalInternalKind, WorkerLockKind};

    #[test]
    fn terminal_failure_preserves_known_domains_and_unknown_fallback() {
        let backend = HalError::IoctlFailed {
            backend: "px4",
            path: Some("/dev/px4video0".into()),
            op: "PTX_SET_CHANNEL",
            errno: 5,
        };
        let cases = [
            (backend, WorkerFailureCategory::BackendControl),
            (HalError::callback_failed("onEvent", "failure"), WorkerFailureCategory::CallbackBinder),
            (HalError::fmq_failed("write", "failure"), WorkerFailureCategory::Fmq),
            (HalError::event_flag_failed("wake", "failure"), WorkerFailureCategory::EventFlag),
            (HalError::cleanup_failed("worker", "failure"), WorkerFailureCategory::Cleanup),
            (HalError::WorkerLockPoisoned { owner: "WorkerRuntime", lock: WorkerLockKind::Wake }, WorkerFailureCategory::Wake),
            (HalError::WorkerLockPoisoned { owner: "frontend", lock: WorkerLockKind::Result }, WorkerFailureCategory::LockPoison),
            (HalError::internal(HalInternalKind::InvariantViolation, "unknown"), WorkerFailureCategory::Unknown),
        ];
        for (error, category) in cases {
            assert_eq!(
                WorkerFailureClassifier::classify_terminal::<()>(
                    WorkerTerminalResult::RuntimeFailure(error.clone()), "panic",
                ),
                ClassifiedWorkerTerminalResult::Failure { category, error },
            );
        }
    }

    #[test]
    fn composed_failure_keeps_primary_domain_and_cleanup_error() {
        let error = HalError::composed_failure(
            "write and cleanup", HalError::fmq_failed("write", "failure"),
            HalError::cleanup_failed("queue", "failure"),
        );
        assert_eq!(
            WorkerFailureClassifier::classify_terminal::<()>(WorkerTerminalResult::RuntimeFailure(error.clone()), "panic"),
            ClassifiedWorkerTerminalResult::Failure { category: WorkerFailureCategory::Fmq, error },
        );
        assert!(matches!(
            WorkerFailureClassifier::classify_terminal::<()>(WorkerTerminalResult::PanicOrJoinFailure, "panic"),
            ClassifiedWorkerTerminalResult::Failure { category: WorkerFailureCategory::Join, .. },
        ));
        assert_eq!(
            WorkerFailureClassifier::classify_terminal::<()>(WorkerTerminalResult::StopRequested, "panic"),
            ClassifiedWorkerTerminalResult::StopRequested,
        );
    }
}
