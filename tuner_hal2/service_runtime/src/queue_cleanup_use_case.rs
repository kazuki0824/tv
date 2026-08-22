use crate::boot::TunerServiceRuntime;
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueCleanupTarget {
    Filter { filter_id: i32 },
    Dvr { dvr_id: i32 },
}

pub(crate) struct QueueCleanupUseCase<'a> {
    runtime: &'a mut TunerServiceRuntime,
    target: QueueCleanupTarget,
}

impl<'a> QueueCleanupUseCase<'a> {
    pub(crate) fn filter(runtime: &'a mut TunerServiceRuntime, filter_id: i32) -> Self {
        Self {
            runtime,
            target: QueueCleanupTarget::Filter { filter_id },
        }
    }

    pub(crate) fn dvr(runtime: &'a mut TunerServiceRuntime, dvr_id: i32) -> Self {
        Self {
            runtime,
            target: QueueCleanupTarget::Dvr { dvr_id },
        }
    }

    pub(crate) fn execute(self) -> Result<(), HalError> {
        match self.target {
            QueueCleanupTarget::Filter { filter_id } => {
                self.runtime.transact_flush_filter_runtime(filter_id)
            }
            QueueCleanupTarget::Dvr { dvr_id } => self.runtime.transact_flush_dvr_runtime(dvr_id),
        }
    }
}
