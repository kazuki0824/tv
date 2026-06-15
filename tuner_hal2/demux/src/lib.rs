//! tuner_hal2 demux層。
//!
//! parser断片は `parser/` 配下に置く。runtime所有は `runtime/` 配下で再構築し、AV shared memory処理は `av/` 配下へ分ける。

pub mod av;
pub mod config;
pub mod parser;
pub mod runtime;

pub use parser::{packet_pipeline, record_index, sections, ts_core};

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
    AvSettings, AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterConfigKind, FilterDelayHint,
    FilterDelayHints, FilterDelayReadiness, FilterOpenType, OpenFilterRequest, PesSettings,
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

    fn pes_start_packet(pid: u16, continuity_counter: u8, payload: &[u8]) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

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
    fn filter_configure_creates_queue_and_reconfigure_clears_old_queue() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                12,
                1,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();

        let (txn, result) = FilterConfigureTxn::new(12).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(100),
                raw: false,
            },
        );

        assert!(result.is_ok());
        assert_eq!(txn.outcome(), Some(FilterConfigureOutcome::Committed));
        assert!(demux.queue_exists(12));

        let (txn, result) = FilterConfigureTxn::new(12).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(101),
                raw: false,
            },
        );

        assert!(result.is_ok());
        assert_eq!(txn.outcome(), Some(FilterConfigureOutcome::Committed));
        assert!(demux.queue_exists(12));
        assert_eq!(demux.filter(12).unwrap().tpid(), Some(101));
    }

    #[test]
    fn started_filter_rejects_reconfigure_and_preserves_state() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                13,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(100),
                    raw: false,
                }),
            ))
            .unwrap();
        demux.start_filter_runtime(13).unwrap();
        let before = demux.filter(13).unwrap().snapshot();

        let (txn, result) = FilterConfigureTxn::new(13).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(101),
                raw: false,
            },
        );

        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateState
            })
        );
        assert_eq!(demux.filter(13).unwrap().snapshot(), before);
    }

    #[test]
    fn filter_start_stop_flush_state_machine() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                14,
                1,
                PipelineOpenKind::Pes,
                Some(FilterPipelineConfig {
                    tpid: Some(200),
                    raw: true,
                }),
            ))
            .unwrap();

        demux.start_filter_runtime(14).unwrap();
        assert_eq!(
            demux.filter(14).unwrap().state(),
            FilterRuntimeState::Started
        );
        demux.start_filter_runtime(14).unwrap();
        assert_eq!(
            demux.filter(14).unwrap().state(),
            FilterRuntimeState::Started
        );
        demux.flush_filter_runtime(14).unwrap();
        assert_eq!(
            demux.filter(14).unwrap().state(),
            FilterRuntimeState::Started
        );
        demux.stop_filter_runtime(14).unwrap();
        assert_eq!(
            demux.filter(14).unwrap().state(),
            FilterRuntimeState::Stopped
        );
        demux.stop_filter_runtime(14).unwrap();
        assert_eq!(
            demux.filter(14).unwrap().state(),
            FilterRuntimeState::Stopped
        );
    }

    #[test]
    fn open_filter_rejects_start_stop_flush_before_configure() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                15,
                1,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();

        assert!(demux.start_filter_runtime(15).is_err());
        assert!(demux.stop_filter_runtime(15).is_err());
        assert!(demux.flush_filter_runtime(15).is_err());
        assert_eq!(demux.filter(15).unwrap().state(), FilterRuntimeState::Open);
    }

    #[test]
    fn av_stream_type_hint_is_stored_and_cleared_by_reconfigure() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                16,
                1,
                PipelineOpenKind::Av,
                Some(FilterPipelineConfig {
                    tpid: Some(300),
                    raw: false,
                }),
            ))
            .unwrap();

        let hint = AvStreamTypeConfig {
            kind: AvStreamKind::Video,
            stream_type: 27,
        };
        demux.configure_filter_av_stream_type(16, hint).unwrap();
        assert_eq!(demux.filter(16).unwrap().av_stream_type_hint(), Some(hint));

        demux
            .configure_filter_runtime(
                16,
                FilterPipelineConfig {
                    tpid: Some(301),
                    raw: false,
                },
            )
            .unwrap();
        assert_eq!(demux.filter(16).unwrap().av_stream_type_hint(), None);
    }

    #[test]
    fn av_configure_uses_av_backing_marker_and_start_stop_preserve_axes() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                18,
                1,
                PipelineOpenKind::Av,
                None,
            ))
            .unwrap();

        demux
            .configure_filter_runtime(
                18,
                FilterPipelineConfig {
                    tpid: Some(400),
                    raw: false,
                },
            )
            .unwrap();
        assert!(demux.filter(18).unwrap().av_backing_present());
        assert!(!demux.filter(18).unwrap().queue_present());
        demux
            .configure_filter_av_stream_type(
                18,
                AvStreamTypeConfig {
                    kind: AvStreamKind::Video,
                    stream_type: 15,
                },
            )
            .unwrap();

        demux.start_filter_runtime(18).unwrap();
        assert_eq!(
            demux.filter(18).unwrap().state(),
            FilterRuntimeState::Started
        );
        assert!(demux.filter(18).unwrap().av_backing_present());
        assert_eq!(
            demux.filter(18).unwrap().av_stream_type_hint(),
            Some(AvStreamTypeConfig {
                kind: AvStreamKind::Video,
                stream_type: 15
            })
        );
        demux.stop_filter_runtime(18).unwrap();
        assert_eq!(
            demux.filter(18).unwrap().state(),
            FilterRuntimeState::Stopped
        );
        assert!(demux.filter(18).unwrap().av_backing_present());
    }

    #[test]
    fn filter_delay_hint_updates_independent_time_and_data_axes() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                17,
                1,
                PipelineOpenKind::Section,
                None,
            ))
            .unwrap();

        demux
            .set_filter_delay_hint(17, FilterDelayHint::TimeDelayMs(10))
            .unwrap();
        demux
            .set_filter_delay_hint(17, FilterDelayHint::DataSizeDelayBytes(188))
            .unwrap();
        let hints = demux.filter(17).unwrap().delay_hints();
        assert_eq!(hints.time_delay_ms, Some(10));
        assert_eq!(hints.data_size_delay_bytes, Some(188));

        demux
            .set_filter_delay_hint(17, FilterDelayHint::TimeDelayMs(0))
            .unwrap();
        demux
            .set_filter_delay_hint(17, FilterDelayHint::DataSizeDelayBytes(0))
            .unwrap();
        let hints = demux.filter(17).unwrap().delay_hints();
        assert_eq!(hints.time_delay_ms, None);
        assert_eq!(hints.data_size_delay_bytes, None);
    }

    #[test]
    fn filter_delay_readiness_uses_or_for_time_and_data_conditions() {
        let hints = FilterDelayHints {
            time_delay_ms: Some(1_000),
            data_size_delay_bytes: Some(188),
        };

        assert_eq!(
            hints.delivery_readiness(999, 187),
            FilterDelayReadiness::WaitingForTime
        );
        assert_eq!(
            hints.delivery_readiness(1_000, 0),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            hints.delivery_readiness(0, 188),
            FilterDelayReadiness::Ready
        );
    }

    #[test]
    fn filter_flush_clears_partial_pes_state_and_keeps_runtime_started() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                22,
                1,
                PipelineOpenKind::Pes,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                22,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                },
            )
            .unwrap();
        demux.start_filter_runtime(22).unwrap();

        let partial_pes = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::Frontend);
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            0x0100,
            22
        )));
        assert!(demux.queue_exists(22));

        demux.flush_filter_runtime(22).unwrap();

        assert_eq!(
            demux.filter(22).unwrap().state(),
            FilterRuntimeState::Started
        );
        assert!(demux.queue_exists(22));
        assert!(!demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            0x0100,
            22
        )));
    }

    #[test]
    fn remove_filter_clears_queue_and_partial_parser_state() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                23,
                1,
                PipelineOpenKind::Pes,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                23,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                },
            )
            .unwrap();
        demux.start_filter_runtime(23).unwrap();

        let partial_pes = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::Frontend);
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            0x0100,
            23
        )));
        assert!(demux.queue_exists(23));

        let removed = demux.remove_filter(23).unwrap();

        assert_eq!(removed.state, FilterRuntimeState::Started);
        assert!(demux.filter(23).is_none());
        assert!(!demux.queue_exists(23));
        assert!(!demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            0x0100,
            23
        )));
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
