use maleicacid_tuner_hal2_common::HalError;

use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
use crate::worker_runtime::WorkerTerminalResult;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerFailureCategory {
    CallbackArtifact,
    CallbackPolicy,
    CallbackConversion,
    CallbackBinder,
    CallbackNotifierTerminal,
    CallbackCleanup,
    Join,
    Unknown,
}

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
                    category: Self::classify_unknown(&error),
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

    pub(crate) const fn classify_unknown(_error: &HalError) -> WorkerFailureCategory {
        WorkerFailureCategory::Unknown
    }
}
