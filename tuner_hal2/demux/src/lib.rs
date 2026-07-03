//! tuner_hal2 demux層。
//!
//! parser断片は `parser/` 配下に置く。runtime所有は `runtime/` 配下で再構築し、AV shared memory処理は `av/` 配下へ分ける。

mod av;
pub mod config;
mod parser;
mod runtime;

pub(crate) use parser::{packet_pipeline, sections, ts_core};

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
        !matches!(self, TsInputOrigin::Playback)
    }
}

pub use av::{
    AvDataId, AvHandleReleaseOutcome, AvMediaEventDescriptor, AvPayloadDeliveryOutcome,
    AvSharedBackingError, AvSharedHandleExport, AvSlotId,
};
pub use config::{
    AvSettings, AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterConfigKind, FilterDelayHint,
    FilterDelayHints, FilterDelayReadiness, FilterOpenType, OpenFilterRequest, PesSettings,
    RecordIndexSettings, SectionCondition, SectionConditionKind,
};
pub use parser::packet_pipeline::{
    PacketDescramblePolicyFailure, PacketPid, PipelineAssemblySuppressionReason,
    PipelineBoundaryReason, PipelineDeliveryAction, PipelineDiagnostic,
    PipelineDiagnosticPidContext, PipelineGeneratedEvent, PipelineReport, PipelineResetReport,
    TsPacketValidationError, ValidatedTsPacket,
};
pub use parser::record_index::{
    supported_record_sc_index_mask, supported_record_ts_index_mask, AVC_SC_B_SLICE, AVC_SC_I_SLICE,
    AVC_SC_P_SLICE, AVC_SC_SI_SLICE, AVC_SC_SP_SLICE, DEMUX_TS_INDEX_ADAPTATION_EXTENSION,
    DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED, DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED,
    DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED, DEMUX_TS_INDEX_DISCONTINUITY,
    DEMUX_TS_INDEX_FIRST_PACKET, DEMUX_TS_INDEX_OPCR, DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
    DEMUX_TS_INDEX_PCR, DEMUX_TS_INDEX_PRIORITY, DEMUX_TS_INDEX_PRIVATE_DATA,
    DEMUX_TS_INDEX_RANDOM_ACCESS, DEMUX_TS_INDEX_SPLICING_POINT, HEVC_SC_AUD, HEVC_SC_BLA_N_LP,
    HEVC_SC_BLA_W_LP, HEVC_SC_BLA_W_RADL, HEVC_SC_IDR_N_LP, HEVC_SC_IDR_W_RADL, HEVC_SC_SPS,
    HEVC_SC_TRAIL_CRA, RECORD_SC_TYPE_NONE, RECORD_SC_TYPE_SC, RECORD_SC_TYPE_SC_AVC,
    RECORD_SC_TYPE_SC_HEVC, RECORD_SC_TYPE_SC_VVC, VVC_SC_AUD, VVC_SC_CRA, VVC_SC_GDR,
    VVC_SC_IDR_N_LP, VVC_SC_IDR_W_RADL, VVC_SC_SPS, VVC_SC_VPS,
};
pub use parser::sections::normalize_length_field_bits;
pub use runtime::{
    configure_dvr_runtime, configure_filter_runtime, DemuxRuntime, DemuxRuntimeError,
    DemuxRuntimeErrorKind, DemuxRuntimeSnapshot, DemuxRuntimeState, DemuxStreamGeneration,
    DvrConfigureOutcome, DvrConfigureReport, DvrConfigureStep, DvrKind, DvrRuntime,
    DvrRuntimeSnapshot, DvrRuntimeState, DvrStatusEvent, FilterConfigureOutcome,
    FilterConfigureReport, FilterConfigureStep, FilterRuntime, FilterRuntimeSnapshot,
    FilterRuntimeState, GenerationBoundaryReport, PlaybackConsumeReport,
    QueueDescriptorExportHandle, QueueDescriptorQueryError, QueueDescriptorSnapshot,
    QueueGrantorDescriptorSnapshot, QueueRuntimeError, QueueRuntimeErrorKind,
};
#[cfg(test)]
pub(crate) use runtime::{
    DvrConfigureTxn, FilterConfigureTxn, SourceBoundaryOutcome, SourceBoundaryStep,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigInputPid;
    use crate::packet_pipeline::{
        FilterPipelineConfig, PacketPid, PipelineBoundaryReason, PipelineOpenKind,
    };
    use crate::runtime::apply_filter_source_boundary_change;
    use crate::runtime::filter::FilterSource;
    use std::os::unix::fs::MetadataExt;
    use std::{thread, time::Duration};

    fn packet_pid(pid: i32) -> PacketPid {
        PacketPid::from_config_pid(ConfigInputPid::validate_tpid(pid).expect("valid test pid"))
    }

    fn pes_start_packet(pid: u16, continuity_counter: u8, payload: &[u8]) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    fn raw_ts_packet(pid: u16, continuity_counter: u8, payload: &[u8]) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    fn open_filter_runtime_with_queue(
        filter_id: i32,
        generation: u64,
        open_type: FilterOpenType,
        config: Option<FilterPipelineConfig>,
    ) -> FilterRuntime {
        DemuxRuntime::open_filter_runtime_from_request(
            filter_id,
            generation,
            &OpenFilterRequest {
                open_type,
                buffer_size: 4096,
                callback_present: false,
            },
            config,
        )
    }

    fn started_section_filter_runtime(filter_id: i32) -> DemuxRuntime {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsSection,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(0x0020),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(filter_id).unwrap();
        demux
    }

    fn first_fd_identity(snapshot: QueueDescriptorSnapshot) -> (u64, u64) {
        let (_grantors, fds, _ints, _quantum, _flags) = snapshot.into_parts();
        let metadata = fds[0].metadata().unwrap();
        (metadata.dev(), metadata.ino())
    }

    #[test]
    fn frontend_and_source_filter_origins_allow_record_mirror() {
        assert!(TsInputOrigin::Frontend.allows_record_mirror());
        assert!(TsInputOrigin::SourceFilter {
            source_filter_id: 1,
            source_filter_generation: 1,
        }
        .allows_record_mirror());
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
        let (report, result) = apply_filter_source_boundary_change(&mut demux, 10, None);
        assert!(result.is_err());
        assert_eq!(
            report.outcome(),
            SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue
            }
        );
        assert!(!report
            .steps()
            .contains(&SourceBoundaryStep::DisconnectDownstream));
        assert!(!report.steps().contains(&SourceBoundaryStep::BumpGeneration));
        assert!(!demux.queue_exists(10));
    }

    #[test]
    fn source_boundary_missing_queue_preserves_existing_source() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                40,
                1,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                41,
                1,
                PipelineOpenKind::Pes,
                None,
            ))
            .unwrap();
        demux.filter_mut(41).unwrap().set_source_filter(40, 1);
        assert!(!demux.queue_exists(41));

        let (report, result) = apply_filter_source_boundary_change(&mut demux, 41, None);

        assert!(result.is_err());
        assert_eq!(
            report.outcome(),
            SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue
            }
        );
        assert!(!report
            .steps()
            .contains(&SourceBoundaryStep::DisconnectDownstream));
        assert_eq!(
            demux.filter(41).unwrap().source(),
            FilterSource::SourceFilter {
                source_filter_id: 40,
                source_filter_generation: 1,
            }
        );
    }

    #[test]
    fn set_filter_source_non_null_uses_source_boundary_txn() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                40,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                41,
                1,
                PipelineOpenKind::Pes,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux.create_filter_queue(41).unwrap();
        assert!(demux.filter(41).unwrap().snapshot().queue_present);

        let reset = demux.set_filter_source_non_null(41, 40).unwrap();

        assert!(reset.cleared);
        let sink = demux.filter(41).unwrap().snapshot();
        assert!(!sink.queue_present);
        assert_eq!(
            sink.source,
            crate::runtime::filter::FilterSource::SourceFilter {
                source_filter_id: 40,
                source_filter_generation: 1,
            }
        );
    }

    #[test]
    fn set_filter_source_non_null_allows_raw_sink_for_ts_linkcap() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                50,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                51,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux.create_filter_queue(51).unwrap();

        let reset = demux.set_filter_source_non_null(51, 50).unwrap();

        assert!(reset.cleared);
        let sink = demux.filter(51).unwrap().snapshot();
        assert_eq!(
            sink.source,
            FilterSource::SourceFilter {
                source_filter_id: 50,
                source_filter_generation: 1,
            }
        );
    }

    #[test]
    fn set_filter_source_non_null_allows_record_sink_for_ts_linkcap() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                52,
                1,
                PipelineOpenKind::Raw,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                53,
                1,
                PipelineOpenKind::Record,
                Some(FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                }),
            ))
            .unwrap();
        demux.create_filter_queue(53).unwrap();

        let reset = demux.set_filter_source_non_null(53, 52).unwrap();

        assert!(reset.cleared);
        let sink = demux.filter(53).unwrap().snapshot();
        assert_eq!(
            sink.source,
            FilterSource::SourceFilter {
                source_filter_id: 52,
                source_filter_generation: 1,
            }
        );
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
    fn raw_filter_queue_desc_is_available_before_configure() {
        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsRaw,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_from_request(
                24, 1, &request, None,
            ))
            .unwrap();

        let snapshot = demux
            .export_filter_queue_descriptor(24)
            .expect("raw filter queue descriptor must exist before configure");

        let (grantors, fds, _ints, quantum, _flags) = snapshot.into_parts();
        assert!(!grantors.is_empty());
        assert!(!fds.is_empty());
        assert!(quantum > 0);
    }

    #[test]
    fn av_filter_queue_desc_is_unavailable() {
        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsVideo,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_from_request(
                25, 1, &request, None,
            ))
            .unwrap();

        assert!(matches!(
            demux.export_filter_queue_descriptor(25),
            Err(QueueDescriptorQueryError::Unavailable(25))
        ));
    }

    #[test]
    fn dvr_queue_desc_requires_configure() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                26,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();

        assert!(matches!(
            demux.export_dvr_queue_descriptor(26),
            Err(QueueDescriptorQueryError::InvalidState(26))
        ));

        demux.configure_dvr_runtime(26).unwrap();
        let snapshot = demux
            .export_dvr_queue_descriptor(26)
            .expect("configured DVR queue descriptor must exist");
        let (grantors, fds, _ints, quantum, _flags) = snapshot.into_parts();
        assert!(!grantors.is_empty());
        assert!(!fds.is_empty());
        assert!(quantum > 0);
    }

    #[test]
    fn raw_filter_queue_desc_preserves_backing_across_reconfigure() {
        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsRaw,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_from_request(
                27, 1, &request, None,
            ))
            .unwrap();
        let first = demux.export_filter_queue_descriptor(27).unwrap();
        let first_identity = first_fd_identity(first);

        FilterConfigureTxn::new(27)
            .configure(
                &mut demux,
                PipelineOpenKind::Raw,
                FilterPipelineConfig {
                    tpid: Some(0x0123),
                    raw: false,
                    record_index: None,
                },
            )
            .1
            .unwrap();
        let second = demux.export_filter_queue_descriptor(27).unwrap();

        assert_eq!(first_identity, first_fd_identity(second));
    }

    #[test]
    fn dvr_queue_desc_preserves_backing_across_reconfigure() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                28,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();

        DvrConfigureTxn::new(28).configure(&mut demux).1.unwrap();
        let first = demux.export_dvr_queue_descriptor(28).unwrap();
        let first_identity = first_fd_identity(first);

        DvrConfigureTxn::new(28).configure(&mut demux).1.unwrap();
        let second = demux.export_dvr_queue_descriptor(28).unwrap();

        assert_eq!(first_identity, first_fd_identity(second));
    }

    #[test]
    fn playback_dvr_start_stop_and_flush_follow_state_machine() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                34,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                34,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(34).unwrap();
        demux
            .register_filter(open_filter_runtime_with_queue(
                36,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                36,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(36).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                37,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(37).unwrap();
        demux.attach_dvr_filter(37, 36).unwrap();
        demux.start_dvr_runtime(37).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                35,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();

        demux.configure_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Configured);

        let first_packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(35, &first_packet)
                .unwrap(),
            188
        );
        assert_eq!(
            demux.consume_playback_dvr_queue(35).unwrap(),
            crate::runtime::PlaybackConsumeReport::default()
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes(34).unwrap(),
            Vec::<u8>::new()
        );

        demux.start_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Started);
        let first_consume = demux.consume_playback_dvr_queue(35).unwrap();
        assert_eq!(first_consume.bytes_read, 188);
        assert_eq!(first_consume.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes(34).unwrap(),
            first_packet.to_vec()
        );
        assert_eq!(
            demux.read_record_dvr_queue_bytes(37).unwrap(),
            Vec::<u8>::new()
        );

        demux.stop_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Stopped);

        let second_packet = raw_ts_packet(0x0100, 1, &[0x05, 0x06, 0x07, 0x08]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(35, &second_packet)
                .unwrap(),
            188
        );
        demux.start_dvr_runtime(35).unwrap();
        let second_consume = demux.consume_playback_dvr_queue(35).unwrap();
        assert_eq!(second_consume.bytes_read, 188);
        assert_eq!(second_consume.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes(34).unwrap(),
            [first_packet.to_vec(), second_packet.to_vec()].concat()
        );

        demux.stop_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Stopped);
        let third_packet = raw_ts_packet(0x0100, 2, &[0x09, 0x0a, 0x0b, 0x0c]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(35, &third_packet)
                .unwrap(),
            188
        );
        demux.flush_dvr_runtime(35).unwrap();
        demux.start_dvr_runtime(35).unwrap();
        let after_flush = demux.consume_playback_dvr_queue(35).unwrap();
        assert_eq!(after_flush.bytes_read, 0);
        assert_eq!(after_flush.completed_packets, 0);
    }

    #[test]
    fn record_dvr_start_succeeds_without_attached_filter() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                35,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();

        demux.configure_dvr_runtime(35).unwrap();

        demux.start_dvr_runtime(35).unwrap();

        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Started);
        assert!(demux.dvr(35).unwrap().attached_record_filters().is_empty());
    }

    #[test]
    fn record_dvr_attach_detach_and_mirror_follow_runtime_contract() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                36,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                36,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(36).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                37,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(37).unwrap();

        demux.attach_dvr_filter(37, 36).unwrap();
        demux.attach_dvr_filter(37, 36).unwrap();
        assert_eq!(demux.dvr(37).unwrap().attached_record_filters().len(), 1);

        demux.start_dvr_runtime(37).unwrap();
        assert_eq!(demux.dvr(37).unwrap().state(), DvrRuntimeState::Started);

        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::Frontend);
        assert!(report
            .delivery_actions
            .contains(&crate::packet_pipeline::PipelineDeliveryAction::DvrMirror { dvr_id: 36 }));
        assert_eq!(
            demux.read_record_dvr_queue_bytes(37).unwrap(),
            packet.to_vec()
        );

        demux.stop_dvr_runtime(37).unwrap();
        assert_eq!(demux.dvr(37).unwrap().state(), DvrRuntimeState::Stopped);
        assert_eq!(
            demux.read_record_dvr_queue_bytes(37).unwrap(),
            Vec::<u8>::new()
        );

        demux.detach_dvr_filter(37, 36).unwrap();
        demux.detach_dvr_filter(37, 36).unwrap();
        assert!(demux.dvr(37).unwrap().attached_record_filters().is_empty());
    }

    #[test]
    fn record_dvr_mirror_overflow_is_diagnostic() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                36,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                36,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(36).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                37,
                1,
                crate::runtime::DvrKind::Record,
                1,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(37).unwrap();
        demux.attach_dvr_filter(37, 36).unwrap();
        demux.start_dvr_runtime(37).unwrap();
        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);

        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::Frontend);

        assert!(report.diagnostics.contains(
            &crate::packet_pipeline::PipelineDiagnostic::RecordDvrMirrorOverflow {
                pid: packet_pid(0x0100),
                source_filter_id: 36,
                dvr_id: 37,
            }
        ));
        assert!(demux.dvr(37).unwrap().pending_overflow());
    }

    #[test]
    fn demux_runtime_malformed_packet_reports_drop_without_accepting_packet() {
        let mut demux = DemuxRuntime::new(1, 1);
        let malformed = [0xffu8; 187];

        let report = demux.push_ts_packet_from_origin(&malformed, TsInputOrigin::Frontend);

        assert_eq!(report.accepted_packets, 0);
        assert_eq!(report.dropped_packets, 1);
        assert_eq!(report.malformed_packets, 1);
        assert!(report
            .drop_reasons
            .contains(&crate::packet_pipeline::PipelineDropReason::MalformedPacket));
        assert!(report
            .diagnostics
            .contains(&crate::packet_pipeline::PipelineDiagnostic::MalformedTsPacket));
    }

    #[test]
    fn record_dvr_attach_rejects_non_record_filter() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                38,
                1,
                PipelineOpenKind::Raw,
                None,
            ))
            .unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                39,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(39).unwrap();

        let error = demux.attach_dvr_filter(39, 38).unwrap_err();
        assert_eq!(
            error.kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidDvrFilter
        );
    }

    #[test]
    fn playback_dvr_rejects_attach_and_detach() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                40,
                1,
                PipelineOpenKind::Record,
                None,
            ))
            .unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                41,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();

        assert_eq!(
            demux.attach_dvr_filter(41, 999).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::UnsupportedDvrOperation,
        );
        assert_eq!(
            demux.detach_dvr_filter(41, 999).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::UnsupportedDvrOperation,
        );

        demux.configure_dvr_runtime(41).unwrap();

        assert_eq!(
            demux.attach_dvr_filter(41, 40).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::UnsupportedDvrOperation,
        );
        assert_eq!(
            demux.detach_dvr_filter(41, 40).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::UnsupportedDvrOperation,
        );
    }

    #[test]
    fn dvr_status_check_interval_hint_updates_without_state_transition() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                42,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();

        demux.set_dvr_status_check_interval(42, 250).unwrap();
        assert_eq!(demux.dvr(42).unwrap().status_check_interval_ms(), 250);
        assert_eq!(demux.dvr(42).unwrap().state(), DvrRuntimeState::Open);

        demux.configure_dvr_runtime(42).unwrap();
        demux.start_dvr_runtime(42).unwrap();
        demux.set_dvr_status_check_interval(42, 500).unwrap();
        assert_eq!(demux.dvr(42).unwrap().status_check_interval_ms(), 500);
        assert_eq!(demux.dvr(42).unwrap().state(), DvrRuntimeState::Started);

        demux.stop_dvr_runtime(42).unwrap();
        demux.set_dvr_status_check_interval(42, 750).unwrap();
        assert_eq!(demux.dvr(42).unwrap().status_check_interval_ms(), 750);
        assert_eq!(demux.dvr(42).unwrap().state(), DvrRuntimeState::Stopped);
    }

    #[test]
    fn dvr_status_events_follow_record_and_playback_threshold_axes() {
        let mut demux = DemuxRuntime::new(1, 1);
        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                420,
                1,
                crate::runtime::DvrKind::Record,
                188,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(420).unwrap();
        demux
            .dvr_mut(420)
            .unwrap()
            .configure_status_reporting(0b1111, 0, 188);
        assert_eq!(
            demux.dvr_status_event(420).unwrap(),
            Some(crate::runtime::DvrStatusEvent::RecordLowWater)
        );
        demux.dvr_mut(420).unwrap().mark_pending_overflow();
        assert_eq!(
            demux.dvr_status_event(420).unwrap(),
            Some(crate::runtime::DvrStatusEvent::RecordOverflow)
        );

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                421,
                1,
                crate::runtime::DvrKind::Playback,
                188,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(421).unwrap();
        demux
            .dvr_mut(421)
            .unwrap()
            .configure_status_reporting(0b1111, 47, 141);
        assert_eq!(
            demux.dvr_status_event(421).unwrap(),
            Some(crate::runtime::DvrStatusEvent::PlaybackSpaceFull)
        );
        assert_eq!(
            demux.write_playback_dvr_queue_bytes(421, &packet).unwrap(),
            188
        );
        assert_eq!(
            demux.dvr_status_event(421).unwrap(),
            Some(crate::runtime::DvrStatusEvent::PlaybackSpaceEmpty)
        );
    }

    #[test]
    fn callback_unhealthy_dvr_rejects_restart() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                422,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(422).unwrap();
        demux.mark_dvr_callback_unhealthy(422).unwrap();
        assert_eq!(
            demux.start_dvr_runtime(422).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );
    }

    #[test]
    fn record_dvr_read_rejects_wrong_kind_or_unconfigured_state() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                43,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        assert_eq!(
            demux.read_record_dvr_queue_bytes(43).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );
        demux.configure_dvr_runtime(43).unwrap();
        assert_eq!(
            demux.read_record_dvr_queue_bytes(43).unwrap(),
            Vec::<u8>::new()
        );

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                44,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(44).unwrap();
        assert_eq!(
            demux.read_record_dvr_queue_bytes(44).unwrap_err().kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );
    }

    #[test]
    fn playback_dvr_write_rejects_wrong_kind_or_unconfigured_state() {
        let mut demux = DemuxRuntime::new(1, 1);
        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                45,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(45, &packet)
                .unwrap_err()
                .kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                46,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(46).unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(46, &packet)
                .unwrap_err()
                .kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );
    }

    #[test]
    fn playback_dvr_write_uses_backpressure_without_eviction() {
        let mut demux = DemuxRuntime::new(1, 1);
        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                47,
                1,
                crate::runtime::DvrKind::Playback,
                64,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(47).unwrap();

        assert_eq!(
            demux.write_playback_dvr_queue_bytes(47, &packet).unwrap(),
            0
        );
    }

    #[test]
    fn playback_dvr_preserves_partial_packet_across_stop_and_clears_it_on_flush() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                48,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                48,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(48).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                49,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(49).unwrap();
        demux.start_dvr_runtime(49).unwrap();

        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(49, &packet[..100])
                .unwrap(),
            100
        );
        let first = demux.consume_playback_dvr_queue(49).unwrap();
        assert_eq!(first.completed_packets, 0);
        assert_eq!(
            demux.snapshot_filter_queue_bytes(48).unwrap(),
            Vec::<u8>::new()
        );

        demux.stop_dvr_runtime(49).unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(49, &packet[100..])
                .unwrap(),
            88
        );
        demux.start_dvr_runtime(49).unwrap();
        let second = demux.consume_playback_dvr_queue(49).unwrap();
        assert_eq!(second.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes(48).unwrap(),
            packet.to_vec()
        );

        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(49, &packet[..100])
                .unwrap(),
            100
        );
        let third = demux.consume_playback_dvr_queue(49).unwrap();
        assert_eq!(third.completed_packets, 0);
        demux.flush_dvr_runtime(49).unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes(49, &packet[100..])
                .unwrap(),
            88
        );
        let after_flush = demux.consume_playback_dvr_queue(49).unwrap();
        assert_eq!(after_flush.completed_packets, 0);
    }

    #[test]
    fn demux_restore_preserves_exported_filter_queue_backing() {
        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsRaw,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter(DemuxRuntime::open_filter_runtime_from_request(
                29, 1, &request, None,
            ))
            .unwrap();
        let before_restore = demux.export_filter_queue_descriptor(29).unwrap();
        let before_identity = first_fd_identity(before_restore);
        let snapshot = demux.snapshot();

        demux.restore(snapshot).unwrap();
        let after_restore = demux.export_filter_queue_descriptor(29).unwrap();

        assert_eq!(before_identity, first_fd_identity(after_restore));
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
                    record_index: None,
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
                record_index: None,
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
                record_index: None,
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
                record_index: None,
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
                    record_index: None,
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
                record_index: None,
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
                    record_index: None,
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
    fn open_filter_stop_is_noop_but_start_and_flush_fail() {
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
        demux.stop_filter_runtime(15).unwrap();
        assert!(demux.flush_filter_runtime(15).is_err());
        assert_eq!(demux.filter(15).unwrap().state(), FilterRuntimeState::Open);
    }

    #[test]
    fn open_dvr_stop_is_noop() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                50,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();

        demux.stop_dvr_runtime(50).unwrap();

        assert_eq!(demux.dvr(50).unwrap().state(), DvrRuntimeState::Open);
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
                    record_index: None,
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
                    record_index: None,
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
                    record_index: None,
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
    fn av_release_fails_when_marker_has_no_runtime_backing() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                19,
                1,
                PipelineOpenKind::Av,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                19,
                FilterPipelineConfig {
                    tpid: Some(400),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        assert!(demux.filter(19).unwrap().av_backing_present());
        assert!(demux.remove_filter_av_backing_for_test(19));

        let error = demux.release_filter_av_handle(19, false, 0).unwrap_err();

        assert_eq!(
            error.kind,
            crate::runtime::DemuxRuntimeErrorKind::AvBackingFailure
        );
    }

    #[test]
    fn av_configure_stream_type_stales_active_slots_and_keeps_backing_exported() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                19,
                1,
                PipelineOpenKind::Av,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                19,
                FilterPipelineConfig {
                    tpid: Some(401),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux
            .mark_filter_av_shared_handle_exported_for_test(19)
            .unwrap();
        let data_id = match demux.allocate_filter_av_payload_for_test(19, 188).unwrap() {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected AV allocation outcome: {other:?}"),
        };
        assert_eq!(demux.filter_av_active_slot_count_for_test(19), Some(1));

        demux
            .configure_filter_av_stream_type(
                19,
                AvStreamTypeConfig {
                    kind: AvStreamKind::Video,
                    stream_type: 27,
                },
            )
            .unwrap();

        assert_eq!(demux.filter_av_active_slot_count_for_test(19), Some(0));
        assert_eq!(
            demux
                .release_filter_av_handle(19, false, data_id.0)
                .unwrap(),
            AvHandleReleaseOutcome::StaleReleaseAccepted { data_id }
        );
        assert!(matches!(
            demux.allocate_filter_av_payload_for_test(19, 188).unwrap(),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }

    #[test]
    fn av_flush_stales_active_slots_without_dropping_shared_handle() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                20,
                1,
                PipelineOpenKind::Av,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                20,
                FilterPipelineConfig {
                    tpid: Some(402),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux
            .mark_filter_av_shared_handle_exported_for_test(20)
            .unwrap();
        let data_id = match demux.allocate_filter_av_payload_for_test(20, 188).unwrap() {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected AV allocation outcome: {other:?}"),
        };

        demux.flush_filter_runtime(20).unwrap();

        assert_eq!(demux.filter_av_active_slot_count_for_test(20), Some(0));
        assert_eq!(
            demux
                .release_filter_av_handle(20, false, data_id.0)
                .unwrap(),
            AvHandleReleaseOutcome::StaleReleaseAccepted { data_id }
        );
        assert!(matches!(
            demux.allocate_filter_av_payload_for_test(20, 188).unwrap(),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }

    #[test]
    fn closed_av_filter_release_rejects_unknown_positive_data_id() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                21,
                1,
                PipelineOpenKind::Av,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                21,
                FilterPipelineConfig {
                    tpid: Some(403),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux
            .mark_filter_av_shared_handle_exported_for_test(21)
            .unwrap();
        let stale_data_id = match demux.allocate_filter_av_payload_for_test(21, 188).unwrap() {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected AV allocation outcome: {other:?}"),
        };
        demux.flush_filter_runtime(21).unwrap();
        let filter = demux.filter_mut(21).unwrap();
        let mut snapshot = filter.snapshot();
        snapshot.state = FilterRuntimeState::Closed;
        filter.restore(snapshot);

        assert_eq!(
            demux.release_filter_av_handle(21, false, 999).unwrap(),
            AvHandleReleaseOutcome::UnknownDataId
        );
        assert_eq!(
            demux
                .release_filter_av_handle(21, false, stale_data_id.0)
                .unwrap(),
            AvHandleReleaseOutcome::StaleReleaseAfterClose {
                data_id: stale_data_id
            }
        );
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
    fn filter_delay_hint_time_only_rearms_after_queue_drain() {
        let mut demux = started_section_filter_runtime(30);
        demux
            .set_filter_delay_hint(30, FilterDelayHint::TimeDelayMs(20))
            .unwrap();

        demux
            .enqueue_filter_queue_payload(30, vec![1, 2, 3])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(30).unwrap(),
            FilterDelayReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness(30).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.drain_filter_queue_for_delivery(30).unwrap(),
            vec![vec![1, 2, 3]]
        );

        demux
            .enqueue_filter_queue_payload(30, vec![4, 5, 6])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(30).unwrap(),
            FilterDelayReadiness::WaitingForTime
        );
    }

    #[test]
    fn filter_delay_hint_data_size_waits_for_threshold() {
        let mut demux = started_section_filter_runtime(31);
        demux
            .set_filter_delay_hint(31, FilterDelayHint::DataSizeDelayBytes(5))
            .unwrap();

        demux
            .enqueue_filter_queue_payload(31, vec![1, 2, 3])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(31).unwrap(),
            FilterDelayReadiness::WaitingForDataSize
        );

        demux.enqueue_filter_queue_payload(31, vec![4, 5]).unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(31).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes(31).unwrap(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn filter_delay_hint_time_and_data_use_or_condition_for_delivery() {
        let mut demux = started_section_filter_runtime(32);
        demux
            .set_filter_delay_hint(32, FilterDelayHint::TimeDelayMs(10_000))
            .unwrap();
        demux
            .set_filter_delay_hint(32, FilterDelayHint::DataSizeDelayBytes(3))
            .unwrap();

        demux
            .enqueue_filter_queue_payload(32, vec![1, 2, 3])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(32).unwrap(),
            FilterDelayReadiness::Ready
        );

        let mut demux = started_section_filter_runtime(33);
        demux
            .set_filter_delay_hint(33, FilterDelayHint::TimeDelayMs(20))
            .unwrap();
        demux
            .set_filter_delay_hint(33, FilterDelayHint::DataSizeDelayBytes(64))
            .unwrap();

        demux
            .enqueue_filter_queue_payload(33, vec![1, 2, 3])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness(33).unwrap(),
            FilterDelayReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness(33).unwrap(),
            FilterDelayReadiness::Ready
        );
    }

    #[test]
    fn push_ts_packet_enqueues_raw_filter_queue_payload_for_delay_gating() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                34,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                34,
                FilterPipelineConfig {
                    tpid: Some(0x0030),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(34).unwrap();
        demux
            .set_filter_delay_hint(34, FilterDelayHint::DataSizeDelayBytes(188))
            .unwrap();

        let packet = raw_ts_packet(0x0030, 0, &[1, 2, 3, 4]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::Frontend);

        assert_eq!(report.accepted_packets, 1);
        assert!(report
            .generated_events
            .contains(&packet_pipeline::PipelineGeneratedEvent::DataReady { filter_id: 34 }));
        assert_eq!(
            demux.filter_delivery_readiness(34).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes(34).unwrap(),
            packet.to_vec()
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
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(22).unwrap();

        let partial_pes = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::Frontend);
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            packet_pid(0x0100),
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
            packet_pid(0x0100),
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
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(23).unwrap();

        let partial_pes = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::Frontend);
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            packet_pid(0x0100),
            23
        )));
        assert!(demux.queue_exists(23));

        let removed = demux.remove_filter(23).unwrap();

        assert_eq!(removed.state, FilterRuntimeState::Started);
        assert!(demux.filter(23).is_none());
        assert!(!demux.queue_exists(23));
        assert!(!demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::Frontend,
            packet_pid(0x0100),
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
                record_index: None,
            },
        );
        assert!(result.is_err());
        assert_eq!(
            demux.filter(30).unwrap().state(),
            FilterRuntimeState::Failed
        );
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
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
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
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
                record_index: None,
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
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
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
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
    }

    #[test]
    fn generation_boundary_overflow_marks_demux_failed() {
        let mut demux = DemuxRuntime::new(1, u64::MAX);
        let result = demux.apply_generation_boundary(PipelineBoundaryReason::TuneStart);
        assert!(result.is_err());
        assert_eq!(demux.state(), DemuxRuntimeState::Failed);
    }

    #[test]
    fn section_generation_overflow_marks_target_filter_failed() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                35,
                1,
                FilterOpenType::TsSection,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                35,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(35).unwrap();
        demux
            .register_filter(open_filter_runtime_with_queue(
                36,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                36,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(36).unwrap();
        let pid = packet_pid(0x0100);
        demux
            .pipeline_mut()
            .section_assembler_generations
            .insert((TsInputOrigin::Frontend, pid), u64::MAX);

        let packet = raw_ts_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0x02]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::Frontend);

        assert!(report.diagnostics.contains(
            &packet_pipeline::PipelineDiagnostic::SectionGenerationOverflow {
                pid,
                filter_ids: vec![35],
            }
        ));
        assert!(!report
            .delivery_actions
            .contains(&packet_pipeline::PipelineDeliveryAction::SectionPayload { filter_id: 35 }));
        assert_eq!(
            demux.filter(35).unwrap().state(),
            FilterRuntimeState::Failed
        );
        assert_eq!(
            demux.filter(36).unwrap().state(),
            FilterRuntimeState::Started
        );
    }

    #[test]
    fn pes_generation_overflow_marks_target_filter_failed_without_av_delivery() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                37,
                1,
                FilterOpenType::TsVideo,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                37,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(37).unwrap();
        let pid = packet_pid(0x0100);
        demux
            .pipeline_mut()
            .pes_assembler_generations
            .insert((TsInputOrigin::Frontend, pid), u64::MAX);

        let packet = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::Frontend);

        assert!(report.diagnostics.contains(
            &packet_pipeline::PipelineDiagnostic::PesGenerationOverflow {
                pid,
                filter_ids: vec![37],
            }
        ));
        assert!(!report
            .delivery_actions
            .contains(&packet_pipeline::PipelineDeliveryAction::AvPayload { filter_id: 37 }));
        assert!(!report.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            packet_pipeline::PipelineDiagnostic::AvSharedHandleNotExported { filter_id: 37, .. }
        )));
        assert_eq!(
            demux.filter(37).unwrap().state(),
            FilterRuntimeState::Failed
        );
    }

    #[test]
    fn generation_boundary_resets_pipeline_and_bumps_generation() {
        let mut demux = DemuxRuntime::new(1, 7);
        let report = demux
            .apply_generation_boundary(PipelineBoundaryReason::TuneStart)
            .unwrap();
        assert_eq!(report.next_generation, DemuxStreamGeneration(8));
        assert_eq!(demux.generation(), 8);
    }
}
