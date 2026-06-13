//! tuner_hal2 demux層。
//!
//! parser断片は `parser/` 配下に置く。runtime所有は `runtime/` 配下で再構築し、AV shared memory処理は `av/` 配下へ分ける。

#[path = "parser/packet_pipeline.rs"]
pub mod packet_pipeline;
#[path = "parser/record_index.rs"]
pub mod record_index;
#[path = "parser/sections.rs"]
pub mod sections;
#[path = "parser/ts_core.rs"]
pub mod ts_core;

pub mod av;
pub mod config;
pub mod parser;
pub mod runtime;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TsInputOrigin {
    Frontend,
    Playback,
    SourceFilter {
        source_filter_id: i32,
        source_filter_generation: u64,
    },
}

impl TsInputOrigin {
    pub const fn allows_record_mirror(self) -> bool {
        matches!(self, TsInputOrigin::Frontend)
    }
}

pub use av::{
    AvDataId, AvDataIdState, AvFilterReleaseState, AvHandleReleaseInput, AvHandleReleaseOutcome,
    AvHandleReleaseTxn, AvPayloadDeliveryOutcome, AvSharedBacking, AvSlotId, ClientHandleState,
};
pub use config::{
    AvSettings, FilterConfig, FilterConfigKind, FilterOpenType, OpenFilterRequest, PesSettings,
    RecordIndexSettings, SectionCondition, SectionConditionKind,
};
pub use runtime::{
    DemuxRuntime, DemuxRuntimeState, DemuxStreamGeneration, DvrConfigureOutcome, DvrConfigureStep,
    DvrConfigureTxn, DvrRuntime, DvrRuntimeState, FilterConfigureOutcome, FilterConfigureStep,
    FilterConfigureTxn, FilterRuntime, FilterRuntimeState, GenerationBoundaryTxn,
    RuntimeIoRegistry, SourceBoundaryOutcome, SourceBoundaryStep, SourceBoundaryTxn,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

    #[test]
    fn frontend_origin_allows_record_mirror() {
        assert!(TsInputOrigin::Frontend.allows_record_mirror());
        assert!(!TsInputOrigin::Playback.allows_record_mirror());
    }

    #[test]
    fn source_boundary_missing_queue_does_not_create_queue() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                10,
                1,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();
        assert!(!demux.queue_exists(10));
        let (txn, result) = SourceBoundaryTxn::new(10).apply(&mut demux);
        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ClearQueue
            })
        );
        assert!(!demux.queue_exists(10));
    }

    #[test]
    fn typed_filter_runtime_preserves_audio_video_open_type() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_typed(
                20,
                1,
                FilterOpenType::TsAudio,
                None,
            ))
            .unwrap();
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_typed(
                21,
                1,
                FilterOpenType::TsVideo,
                None,
            ))
            .unwrap();

        assert_eq!(
            demux.filter(20).unwrap().open_type(),
            FilterOpenType::TsAudio
        );
        assert_eq!(demux.filter(20).unwrap().open_kind(), PipelineOpenKind::Av);
        assert_eq!(
            demux.filter(21).unwrap().open_type(),
            FilterOpenType::TsVideo
        );
        assert_eq!(demux.filter(21).unwrap().open_kind(), PipelineOpenKind::Av);
    }

    #[test]
    fn open_filter_runtime_preserves_request_boundary() {
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsSection,
            buffer_size: 4096,
            callback_present: true,
        };
        let filter = DemuxRuntime::open_filter_runtime_from_request(22, 1, &request, None);

        assert_eq!(filter.open_type(), FilterOpenType::TsSection);
        assert_eq!(filter.open_kind(), PipelineOpenKind::Section);
        assert_eq!(filter.buffer_size(), 4096);
        assert!(filter.callback_present());
    }

    #[test]
    fn open_dvr_runtime_preserves_request_boundary() {
        let dvr =
            DemuxRuntime::open_dvr_runtime(23, 1, crate::runtime::DvrKind::Playback, 8192, true);

        assert_eq!(dvr.kind(), crate::runtime::DvrKind::Playback);
        assert_eq!(dvr.buffer_size(), 8192);
        assert!(dvr.callback_present());
        assert!(dvr.playback_assembler_present());
    }

    #[test]
    fn filter_configure_failure_rolls_back_runtime_snapshot() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                11,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(100),
                    raw: false,
                }),
            ))
            .unwrap();
        demux.create_filter_queue(11).unwrap();
        let before = demux.filter(11).unwrap().snapshot();
        let (txn, result) = FilterConfigureTxn::new(11).configure(
            &mut demux,
            PipelineOpenKind::Pes,
            FilterPipelineConfig {
                tpid: Some(101),
                raw: true,
            },
        );
        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateSettings
            })
        );
        assert_eq!(demux.filter(11).unwrap().snapshot(), before);
    }

    #[test]
    fn generation_overflow_marks_filter_failed() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                30,
                u64::MAX,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();
        let result = demux.configure_filter_runtime(
            30,
            FilterPipelineConfig {
                tpid: Some(100),
                raw: false,
            },
        );
        assert!(result.is_err());
        assert_eq!(
            demux.filter(30).unwrap().state(),
            FilterRuntimeState::Failed
        );
    }

    #[test]
    fn generation_overflow_marks_dvr_failed() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_record_dvr_runtime(31, u64::MAX))
            .unwrap();
        let result = demux.configure_dvr_runtime(31);
        assert!(result.is_err());
        assert_eq!(demux.dvr(31).unwrap().state(), DvrRuntimeState::Failed);
    }

    #[test]
    fn filter_configure_txn_does_not_rollback_generation_overflow() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                32,
                u64::MAX,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();
        let (txn, result) = FilterConfigureTxn::new(32).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(100),
                raw: false,
            },
        );
        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(FilterConfigureOutcome::Failed {
                failed_step: FilterConfigureStep::ApplySoftDemuxConfig
            })
        );
        assert_eq!(
            demux.filter(32).unwrap().state(),
            FilterRuntimeState::Failed
        );
    }

    #[test]
    fn dvr_configure_txn_does_not_rollback_generation_overflow() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_record_dvr_runtime(33, u64::MAX))
            .unwrap();
        let (txn, result) = DvrConfigureTxn::new(33).configure(&mut demux);
        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(DvrConfigureOutcome::Failed {
                failed_step: DvrConfigureStep::ApplySoftDemuxConfig
            })
        );
        assert_eq!(demux.dvr(33).unwrap().state(), DvrRuntimeState::Failed);
    }

    #[test]
    fn generation_boundary_overflow_marks_demux_failed() {
        let mut demux = DemuxRuntime::new(1, u64::MAX);
        let (_, result) =
            GenerationBoundaryTxn::new(DemuxStreamGeneration(u64::MAX)).apply(&mut demux);
        assert!(result.is_err());
        assert_eq!(demux.state(), DemuxRuntimeState::Failed);
    }

    #[test]
    fn generation_boundary_resets_pipeline_and_bumps_generation() {
        let mut demux = DemuxRuntime::new(1, 7);
        let (_, report) = GenerationBoundaryTxn::new(DemuxStreamGeneration(7)).apply(&mut demux);
        let report = report.unwrap();
        assert_eq!(report.next_generation, DemuxStreamGeneration(8));
        assert_eq!(demux.generation(), 8);
    }
}
