use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind,
};

pub(crate) fn demux_runtime_error_to_hal(
    error: maleicacid_tuner_hal2_demux::DemuxRuntimeError,
) -> HalError {
    match error.kind {
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::GenerationExhausted => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime generation exhausted",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FilterMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::DvrMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueMissing => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime object is missing",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidState
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidDvrFilter
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceLifecycle
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SinkLifecycle => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime lifecycle is invalid",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSourceSubtype
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
            HalError::Unsupported("demux source/sink subtype is unsupported")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
            HalError::Unsupported("DVR operation is unavailable for this DVR kind")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PidMismatch => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "demux source/sink PID mismatch",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SelfReference => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "a filter cannot use itself as its data source",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PipelineFailed
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::RelationCommitUnknown => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime pipeline or relation operation failed",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
            HalError::cleanup_failed(
                "demux source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailureRollbackFailed => {
            HalError::cleanup_failed(
                "playback queue read rollback",
                "DVR was quarantined after playback queue transaction rollback failure",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FmqDeliveryFailed(kind) => {
            HalError::FmqDeliveryFailed {
                kind,
                object_id: error.id,
            }
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FmqDeliveryRollbackFailed {
            delivery,
            rollback,
        } => compose_primary_cleanup_failure(
            "FMQ delivery and playback queue rollback failed",
            HalError::FmqDeliveryFailed {
                kind: delivery,
                object_id: error.id,
            },
            HalError::cleanup_failed(
                "playback queue read rollback",
                format!("{:?}: {}", rollback.kind, rollback.detail),
            ),
        ),
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailureWithContext(
            context,
        ) => match context.kind {
            maleicacid_tuner_hal2_demux::QueueRuntimeErrorKind::GateLockPoisoned {
                lock: maleicacid_tuner_hal2_demux::QueueRuntimeLockKind::FilterProducerDrainGateData,
                poison_count,
                counter_saturated,
                producer_release,
                drain_rollback,
            } => HalError::FilterGateLockPoisoned {
                filter_id: error.id,
                poison_count,
                counter_saturated,
                producer_release,
                drain_rollback,
            },
            _ => HalError::internal(HalInternalKind::InvariantViolation, context.detail),
        },
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailure
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::AvBackingFailure => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime queue operation failed",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use maleicacid_tuner_hal2_common::HalError;

    #[test]
    fn filter_gate_poison_reaches_service_error_without_losing_context() {
        use maleicacid_tuner_hal2_demux::{
            DemuxRuntimeError, QueueRuntimeError, QueueRuntimeErrorKind, QueueRuntimeLockKind,
        };
        for (poison_count, counter_saturated) in [(7, false), (u64::MAX >> 3, true)] {
            let context = QueueRuntimeError {
                kind: QueueRuntimeErrorKind::GateLockPoisoned {
                    lock: QueueRuntimeLockKind::FilterProducerDrainGateData,
                    poison_count,
                    counter_saturated,
                    producer_release: true,
                    drain_rollback: true,
                },
                detail: "filter gate data lock poisoned",
            };
            let error = super::demux_runtime_error_to_hal(DemuxRuntimeError::queue_runtime_error(
                17, context,
            ));
            assert_eq!(
                error,
                HalError::FilterGateLockPoisoned {
                    filter_id: Some(17),
                    poison_count,
                    counter_saturated,
                    producer_release: true,
                    drain_rollback: true,
                }
            );
        }
    }
}
