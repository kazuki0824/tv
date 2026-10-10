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
                "demux runtime世代を発行できません",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FilterMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::DvrMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueMissing => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtimeオブジェクトがありません",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidState
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidDvrFilter
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceLifecycle
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SinkLifecycle => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtimeのライフサイクルが不正です",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSourceSubtype
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
            HalError::Unsupported("demux source/sinkのsubtypeは未対応です")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
            HalError::Unsupported("このDVR種別ではDVR操作を利用できません")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PidMismatch => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "demux source/sinkのPIDが一致しません",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SelfReference => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "filter自身をデータソースに指定できません",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PipelineFailed
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::RelationCommitUnknown => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtimeのpipelineまたはrelation操作に失敗しました",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
            HalError::cleanup_failed(
                "demux source boundaryの巻戻し",
                "source boundary巻戻し失敗後にdemux runtimeを隔離しました",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailureWithRollback {
            primary,
            rollback,
        } => compose_primary_cleanup_failure(
            "DVRキュー操作と巻戻しがともに失敗しました",
            queue_error_to_hal(error.id, primary),
            queue_error_to_hal(error.id, rollback),
        ),
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
            "FMQ配送とplayback queue巻戻しがともに失敗しました",
            HalError::FmqDeliveryFailed {
                kind: delivery,
                object_id: error.id,
            },
            queue_error_to_hal(error.id, rollback),
        ),
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailureWithContext(
            context,
        ) => queue_error_to_hal(error.id, context),
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailure
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::AvBackingFailure => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime queue操作に失敗しました",
            )
        }
    }
}

fn queue_error_to_hal(
    id: Option<i32>,
    context: maleicacid_tuner_hal2_demux::QueueRuntimeError,
) -> HalError {
    match context.kind {
        maleicacid_tuner_hal2_demux::QueueRuntimeErrorKind::EpochLockPoisoned(poison) => {
            HalError::QueueEpochLockPoisoned { dvr_id: id, poison }
        }

        maleicacid_tuner_hal2_demux::QueueRuntimeErrorKind::GateLockPoisoned {
            lock: maleicacid_tuner_hal2_demux::QueueRuntimeLockKind::FilterProducerDrainGateData,
            poison_count,
            counter_saturated,
            producer_release,
            drain_rollback,
        } => HalError::FilterGateLockPoisoned {
            filter_id: id,
            poison_count,
            counter_saturated,
            producer_release,
            drain_rollback,
        },
        _ => HalError::internal(HalInternalKind::InvariantViolation, context.detail),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn epoch_poison_survives_primary_and_cleanup_projection() {
        use maleicacid_tuner_hal2_common::{LockPoisonDiagnostic, RuntimeLockKind};
        use maleicacid_tuner_hal2_demux::{
            DemuxRuntimeError, DemuxRuntimeErrorKind, QueueRuntimeError, QueueRuntimeErrorKind,
        };
        let primary = LockPoisonDiagnostic {
            lock: RuntimeLockKind::DvrQueueEpoch {
                queue_identity: Some(77),
            },
            poison_count: 1,
            counter_saturated: false,
        };
        let cleanup = LockPoisonDiagnostic {
            poison_count: 2,
            ..primary
        };
        let wrap = |poison| QueueRuntimeError {
            kind: QueueRuntimeErrorKind::EpochLockPoisoned(poison),
            detail: "汚染",
        };
        let simple = super::demux_runtime_error_to_hal(DemuxRuntimeError::queue_runtime_error(
            7,
            wrap(primary),
        ));
        assert_eq!(
            simple,
            HalError::QueueEpochLockPoisoned {
                dvr_id: Some(7),
                poison: primary
            }
        );
        let error = super::demux_runtime_error_to_hal(DemuxRuntimeError {
            kind: DemuxRuntimeErrorKind::QueueRuntimeFailureWithRollback {
                primary: wrap(primary),
                rollback: wrap(cleanup),
            },
            id: Some(7),
        });
        assert_eq!(
            error.primary_error(),
            &HalError::QueueEpochLockPoisoned {
                dvr_id: Some(7),
                poison: primary
            }
        );
        assert_eq!(
            error.cleanup_error(),
            Some(&HalError::QueueEpochLockPoisoned {
                dvr_id: Some(7),
                poison: cleanup
            })
        );
    }

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
                detail: "filter gateデータロックが汚染されています",
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
