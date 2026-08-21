use maleicacid_tuner_hal2_common::HalError;

use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerFailureCategory {
    CallbackArtifact,
    CallbackPolicy,
    CallbackConversion,
    CallbackBinder,
    CallbackNotifierTerminal,
    CallbackCleanup,
    StopSignal,
    Wake,
    Join,
    EventFlag,
    Reaper,
    BackendControl,
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

pub(crate) struct WorkerFailureClassifier;

impl WorkerFailureClassifier {
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
            CallbackDeliveryFailurePhase::NotifierCleanup => {
                WorkerFailureCategory::CallbackCleanup
            }
        };
        ClassifiedCallbackFailure { report, category }
    }

    pub(crate) const fn classify_stop_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::StopSignal
    }

    pub(crate) const fn classify_wake_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::Wake
    }

    pub(crate) const fn classify_join_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::Join
    }

    pub(crate) const fn classify_event_flag_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::EventFlag
    }

    pub(crate) const fn classify_reaper_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::Reaper
    }

    pub(crate) const fn classify_backend_control_failure() -> WorkerFailureCategory {
        WorkerFailureCategory::BackendControl
    }

    pub(crate) const fn classify_unknown(_error: &HalError) -> WorkerFailureCategory {
        WorkerFailureCategory::Unknown
    }
}
