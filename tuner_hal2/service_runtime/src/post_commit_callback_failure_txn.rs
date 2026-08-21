use maleicacid_tuner_hal2_common::HalError;

use crate::boot::TunerServiceRuntime;
use crate::boot::CallbackDeliveryFailureReport;
use crate::worker_failure_classifier::{
    ClassifiedCallbackFailure, WorkerFailureCategory,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallbackHealthEffect {
    Preserve,
    MarkUnhealthy,
}

fn callback_health_effect(
    report: &CallbackDeliveryFailureReport,
    category: WorkerFailureCategory,
) -> CallbackHealthEffect {
    let preserve = match report {
        CallbackDeliveryFailureReport::Dvr { .. } => matches!(
            category,
            WorkerFailureCategory::CallbackArtifact
                | WorkerFailureCategory::CallbackPolicy
                | WorkerFailureCategory::CallbackCleanup
        ),
        CallbackDeliveryFailureReport::Filter { .. }
        | CallbackDeliveryFailureReport::FrontendEvent { .. }
        | CallbackDeliveryFailureReport::FrontendScanEnd { .. } => {
            category == WorkerFailureCategory::CallbackArtifact
        }
    };
    if preserve {
        CallbackHealthEffect::Preserve
    } else {
        CallbackHealthEffect::MarkUnhealthy
    }
}

pub(crate) struct PostCommitCallbackFailureTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl<'a> PostCommitCallbackFailureTxn<'a> {
    pub(crate) fn new(runtime: &'a mut TunerServiceRuntime) -> Self {
        Self { runtime }
    }

    pub(crate) fn execute(
        self,
        classified: ClassifiedCallbackFailure,
    ) -> Result<(), HalError> {
        let (report, category) = classified.into_parts();
        let health_effect = callback_health_effect(&report, category);
        self.runtime
            .commit_post_callback_failure_effects(report, health_effect)
    }
}
