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
    Frontend {
        frontend_generation: u64,
    },
    PlaybackDvr {
        dvr_id: i32,
        queue_identity: u64,
        queue_epoch: u64,
    },
    SourceFilter {
        source_filter_id: i32,
        source_filter_generation: u64,
    },
}

impl TsInputOrigin {
    pub const fn frontend(frontend_generation: u64) -> Self {
        Self::Frontend {
            frontend_generation,
        }
    }

    pub const fn playback_dvr(dvr_id: i32, queue_identity: u64, queue_epoch: u64) -> Self {
        Self::PlaybackDvr {
            dvr_id,
            queue_identity,
            queue_epoch,
        }
    }
}

pub use av::{
    AvDataId, AvDataIdAllocator, AvFileIdentity, AvHandleReleaseDescriptor,
    AvHandleReleaseOutcome, AvMediaEventDescriptor, AvPayloadDeliveryOutcome, AvRuntimeBudget,
    AvSharedBacking, AvSharedBackingError, AvSharedHandleExport, AvSlotId,
    DEFAULT_AV_MAX_EVENT_BYTES, DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
    DEFAULT_AV_PER_FILTER_LIVE_BYTES,
};
pub use config::{
    AvSettings, AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterConfigKind, FilterDelayHint,
    FilterDelayHints, FilterDelayReadiness, FilterOpenType, OpenFilterRequest, PesSettings,
    RecordIndexSettings, SectionCondition, SectionConditionKind, PES_STREAM_ID_WILDCARD,
};
pub use parser::packet_pipeline::{
    PacketDescramblePolicyFailure, PacketPid, PipelineAssemblySuppressionReason,
    PipelineBoundaryReason, PipelineDeliveryAction, PipelineDiagnostic,
    PipelineDiagnosticCounters, PipelineDiagnosticPidContext, PipelineGeneratedEvent,
    PipelineReport, PipelineResetReport, TsPacketValidationError, ValidatedTsPacket,
};
pub use parser::ts_core::MAX_PES_BUFFER_BYTES;
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
    RECORD_SC_TYPE_SC_HEVC, RECORD_SC_TYPE_SC_VVC, TsRecordEventData, VVC_SC_AUD, VVC_SC_CRA,
    VVC_SC_GDR, VVC_SC_IDR_N_LP, VVC_SC_IDR_W_RADL, VVC_SC_SPS, VVC_SC_VPS,
};
pub use parser::sections::normalize_length_field_bits;
pub use runtime::{
    CommittedFilterQueueCleanup, DemuxStreamBoundaryRequest, DemuxRuntime, DemuxRuntimeError,
    DemuxRuntimeErrorKind,
    DemuxRuntimeQuarantineRequest, DemuxRuntimeRollbackCommitRequest,
    DemuxRuntimeRollbackRestoreRequest, DemuxRuntimeRollbackToken,
    DemuxRuntimeRollbackTokenPrepareRequest, DemuxRuntimeSnapshot, DemuxRuntimeState,
    DemuxStreamGeneration, DvrConfigureOutcome, DvrConfigureReport, DvrConfigureStep,
    DvrDataFormat, DvrFilterLinkRequest, DvrKind, DvrRuntimeConfigureRequest,
    DvrRuntimeOperationRequest, DvrRuntimeRegistrationRequest, DvrRuntimeSnapshot,
    DvrRuntimeState, DvrStatusEvent, DvrStatusIntervalRuntimeRequest, DvrStatusReportingRequest,
    FilterAvHandleReleaseRequest,
    FilterAvStreamTypeRuntimeRequest, FilterConfigureOutcome, FilterConfigureReport,
    FilterConfigureStep, FilterDelayHintRuntimeRequest, FilterRuntimeConfigureRequest,
    FilterQueueCleanupPlan, FilterQueuePayloadCleanupOutcome,
    FilterRuntimeOperationKind, FilterRuntimeOperationOutcome, FilterRuntimeOperationReport,
    FilterRuntimeOperationRequest, FilterRuntimeOperationSkipReason, FilterRuntimeOperationStep,
    FilterRuntimeOperationStepOutcome, FilterRuntimeRegistrationRequest, FilterRuntimeSnapshot,
    FilterRuntimeState, FilterSourceConnectRequest, FilterSourceDisconnectRequest,
    StreamBoundaryReport, PlaybackConsumeReport, PlaybackFlushDiagnostic,
    PlaybackQueueReadTxn, PlaybackStats, PreparedDvrFilterRelation,
    PreparedStreamBoundary,
    QueueDescriptorExportPlan, QueueDescriptorExportTarget, QueueDescriptorQueryError,
    QueueDescriptorSnapshot, QueueGrantorDescriptorSnapshot, QueueRuntimeError,
    QueueRuntimeErrorKind, RecordDvrFilterRelationState, SourceBoundaryOutcome,
    SourceBoundaryReport, SourceBoundaryStep,
    FilterStatusEvent, WatermarkClassifier, WatermarkDecision, WatermarkPolicy,
    WatermarkQueueSnapshot,
    ValidatedPacketIngressRequest,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigInputPid;
    use crate::packet_pipeline::{
        FilterPipelineConfig, PacketPid, PipelineBoundaryReason, PipelineOpenKind,
    };
    use crate::runtime::configure_txn::{DvrConfigureTxn, FilterConfigureTxn};
    use crate::runtime::filter::FilterRuntime;
    use crate::runtime::filter::FilterSource;
    use crate::runtime::source_boundary::{
        apply_filter_source_boundary_change, SourceBoundaryOutcome, SourceBoundaryStep,
    };
    use std::os::unix::fs::MetadataExt;
    use std::{thread, time::Duration};

    trait DemuxRuntimeTestMutationExt {
        fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError>;
        fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError>;
        fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError>;
        fn configure_filter_av_stream_type(
            &mut self,
            filter_id: i32,
            config: AvStreamTypeConfig,
        ) -> Result<(), DemuxRuntimeError>;
        fn set_filter_delay_hint(
            &mut self,
            filter_id: i32,
            hint: FilterDelayHint,
        ) -> Result<(), DemuxRuntimeError>;
        fn release_filter_av_handle(
            &mut self,
            filter_id: i32,
            descriptor: AvHandleReleaseDescriptor,
            av_data_id: i64,
        ) -> Result<AvHandleReleaseOutcome, DemuxRuntimeError>;
        fn set_dvr_status_check_interval(
            &mut self,
            dvr_id: i32,
            interval_ms: u64,
        ) -> Result<(), DemuxRuntimeError>;
        fn mark_dvr_callback_unhealthy(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError>;
        fn attach_dvr_filter(
            &mut self,
            dvr_id: i32,
            filter_id: i32,
        ) -> Result<(), DemuxRuntimeError>;
        fn detach_dvr_filter(
            &mut self,
            dvr_id: i32,
            filter_id: i32,
        ) -> Result<(), DemuxRuntimeError>;
        fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError>;
        fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError>;
        fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError>;
        fn set_filter_source_non_null(
            &mut self,
            sink_filter_id: i32,
            source_filter_id: i32,
        ) -> Result<PipelineResetReport, DemuxRuntimeError>;
        fn remove_filter(
            &mut self,
            filter_id: i32,
        ) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError>;
    }

    impl DemuxRuntimeTestMutationExt for DemuxRuntime {
        fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
            self.start_filter_runtime(filter_id)
        }
        fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
            self.stop_filter_runtime(filter_id)
        }
        fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
            self.flush_filter_runtime(filter_id)
        }
        fn configure_filter_av_stream_type(
            &mut self,
            filter_id: i32,
            config: AvStreamTypeConfig,
        ) -> Result<(), DemuxRuntimeError> {
            self.configure_filter_av_stream_type(filter_id, config)
        }
        fn set_filter_delay_hint(
            &mut self,
            filter_id: i32,
            hint: FilterDelayHint,
        ) -> Result<(), DemuxRuntimeError> {
            self.set_filter_delay_hint(filter_id, hint)
        }
        fn release_filter_av_handle(
            &mut self,
            filter_id: i32,
            descriptor: AvHandleReleaseDescriptor,
            av_data_id: i64,
        ) -> Result<AvHandleReleaseOutcome, DemuxRuntimeError> {
            self.release_filter_av_handle(filter_id, descriptor, av_data_id)
        }
        fn set_dvr_status_check_interval(
            &mut self,
            dvr_id: i32,
            interval_ms: u64,
        ) -> Result<(), DemuxRuntimeError> {
            self.set_dvr_status_check_interval(dvr_id, interval_ms)
        }
        fn mark_dvr_callback_unhealthy(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
            self.mark_dvr_callback_unhealthy(dvr_id)
        }
        fn attach_dvr_filter(
            &mut self,
            dvr_id: i32,
            filter_id: i32,
        ) -> Result<(), DemuxRuntimeError> {
            self.attach_dvr_filter(dvr_id, filter_id)
        }
        fn detach_dvr_filter(
            &mut self,
            dvr_id: i32,
            filter_id: i32,
        ) -> Result<(), DemuxRuntimeError> {
            self.detach_dvr_filter(dvr_id, filter_id)
        }
        fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
            self.start_dvr_runtime(dvr_id)
        }
        fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
            self.stop_dvr_runtime(dvr_id)
        }
        fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
            self.flush_dvr_runtime(dvr_id)
        }
        fn set_filter_source_non_null(
            &mut self,
            sink_filter_id: i32,
            source_filter_id: i32,
        ) -> Result<PipelineResetReport, DemuxRuntimeError> {
            self.set_filter_source_non_null(sink_filter_id, source_filter_id)
                .1
        }
        fn remove_filter(
            &mut self,
            filter_id: i32,
        ) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
            self.remove_filter(filter_id)
        }
    }

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

    fn pcr_packet(pid: u16, continuity_counter: u8, pcr_base_90khz: u64) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x20 | (continuity_counter & 0x0f);
        packet[4] = 7;
        packet[5] = 0x10;
        packet[6] = (pcr_base_90khz >> 25) as u8;
        packet[7] = (pcr_base_90khz >> 17) as u8;
        packet[8] = (pcr_base_90khz >> 9) as u8;
        packet[9] = (pcr_base_90khz >> 1) as u8;
        packet[10] = ((pcr_base_90khz & 1) as u8) << 7 | 0x7e;
        packet[11] = 0;
        packet
    }

    fn pcr_payload_packet(
        pid: u16,
        continuity_counter: u8,
        pcr_base_90khz: u64,
    ) -> [u8; 188] {
        let mut packet = pcr_packet(pid, continuity_counter, pcr_base_90khz);
        packet[3] = 0x30 | (continuity_counter & 0x0f);
        packet
    }

    fn discontinuity_packet_without_pcr(pid: u16, continuity_counter: u8) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x20 | (continuity_counter & 0x0f);
        packet[4] = 1;
        packet[5] = 0x80;
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

    #[test]
    fn pcr_clock_anchor_advances_and_flush_invalidates_it() {
        let filter_id = 71;
        let pid = 0x0100;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(pid),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        assert!(!demux.queue_exists(filter_id));
        demux.start_filter_runtime(filter_id).unwrap();

        let base = 90_000;
        let report = demux.push_ts_packet_from_origin(
            &pcr_packet(pid as u16, 0, base),
            TsInputOrigin::frontend(1),
        );
        assert_eq!(report.accepted_packets, 1);
        assert!(report.generated_events.is_empty());
        assert!(demux.pcr_clock_time_90khz(filter_id).is_some_and(|value| value >= base));

        demux.flush_filter_runtime(filter_id).unwrap();
        assert_eq!(demux.pcr_clock_time_90khz(filter_id), None);
    }

    #[test]
    fn pcr_discontinuity_requires_a_following_clean_anchor() {
        let filter_id = 72;
        let pid = 0x0100;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(pid),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(filter_id).unwrap();

        demux.push_ts_packet_from_origin(
            &pcr_packet(pid as u16, 0, 90_000),
            TsInputOrigin::frontend(1),
        );
        assert!(demux.pcr_clock_time_90khz(filter_id).is_some());

        let mut discontinuity = pcr_packet(pid as u16, 1, 180_000);
        discontinuity[5] |= 0x80;
        demux.push_ts_packet_from_origin(&discontinuity, TsInputOrigin::frontend(1));
        assert_eq!(demux.pcr_clock_time_90khz(filter_id), None);

        demux.push_ts_packet_from_origin(
            &pcr_packet(pid as u16, 2, 270_000),
            TsInputOrigin::frontend(1),
        );
        assert!(demux
            .pcr_clock_time_90khz(filter_id)
            .is_some_and(|value| value >= 270_000));
    }

    #[test]
    fn pcr_discontinuity_without_pcr_invalidates_anchor() {
        let filter_id = 75;
        let pid = 0x0100;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(pid),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(filter_id).unwrap();
        demux.push_ts_packet_from_origin(
            &pcr_packet(pid as u16, 0, 90_000),
            TsInputOrigin::frontend(1),
        );
        assert!(demux.pcr_clock_time_90khz(filter_id).is_some());

        demux.push_ts_packet_from_origin(
            &discontinuity_packet_without_pcr(pid as u16, 1),
            TsInputOrigin::frontend(1),
        );

        assert_eq!(demux.pcr_clock_time_90khz(filter_id), None);
    }

    #[test]
    fn duplicate_and_tei_pcr_packets_do_not_replace_clean_anchor() {
        let filter_id = 76;
        let pid = 0x0100;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(pid),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(filter_id).unwrap();
        let first = pcr_payload_packet(pid as u16, 0, 90_000);
        demux.push_ts_packet_from_origin(&first, TsInputOrigin::frontend(1));
        let clean_anchor = demux
            .pcr_anchor_observation_for_test(filter_id)
            .unwrap();

        demux.push_ts_packet_from_origin(&first, TsInputOrigin::frontend(1));
        assert_eq!(
            demux.pcr_anchor_observation_for_test(filter_id),
            Some(clean_anchor)
        );
        let mut tei = pcr_payload_packet(pid as u16, 1, 1_800_000);
        tei[1] |= 0x80;
        demux.push_ts_packet_from_origin(&tei, TsInputOrigin::frontend(1));

        assert_eq!(
            demux.pcr_anchor_observation_for_test(filter_id),
            Some(clean_anchor)
        );
        assert!(demux
            .pcr_clock_time_90khz(filter_id)
            .is_some_and(|value| (90_000..450_000).contains(&value)));
    }

    #[test]
    fn playback_dvr_configure_preserves_and_flush_invalidates_pcr_clock_anchor() {
        let filter_id = 73;
        let dvr_id = 74;
        let pid = 0x0100;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                filter_id,
                FilterPipelineConfig {
                    tpid: Some(pid),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(filter_id).unwrap();
        demux.push_ts_packet_from_origin(
            &pcr_packet(pid as u16, 0, 90_000),
            TsInputOrigin::frontend(1),
        );
        assert!(demux.pcr_clock_time_90khz(filter_id).is_some());

        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                dvr_id,
                1,
                crate::runtime::DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(dvr_id).unwrap();

        assert!(demux.pcr_clock_time_90khz(filter_id).is_some());
        demux.flush_dvr_runtime(dvr_id).unwrap();
        assert_eq!(demux.pcr_clock_time_90khz(filter_id), None);
    }

    #[test]
    fn av_sync_id_tables_allow_many_media_filters_and_remove_only_the_target_relation() {
        let pcr_filter_id = 77;
        let audio_filter_id = 78;
        let video_filter_id = 79;
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                pcr_filter_id,
                1,
                FilterOpenType::TsPcr,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                pcr_filter_id,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        for (filter_id, open_type, tpid) in [
            (audio_filter_id, FilterOpenType::TsAudio, 0x0101),
            (video_filter_id, FilterOpenType::TsVideo, 0x0102),
        ] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    1,
                    open_type,
                    None,
                ))
                .unwrap();
            demux
                .configure_filter_runtime(
                    filter_id,
                    FilterPipelineConfig {
                        tpid: Some(tpid),
                        raw: false,
                        record_index: None,
                    },
                )
                .unwrap();
        }

        assert_eq!(
            demux.av_sync_hw_id_for_media_filter(audio_filter_id),
            Some(pcr_filter_id)
        );
        assert_eq!(
            demux.av_sync_hw_id_for_media_filter(video_filter_id),
            Some(pcr_filter_id)
        );
        assert_eq!(
            demux.pcr_filter_id_for_av_sync_hw_id(pcr_filter_id),
            Some(pcr_filter_id)
        );

        demux.remove_filter(audio_filter_id).unwrap();
        assert_eq!(demux.av_sync_hw_id_for_media_filter(audio_filter_id), None);
        assert_eq!(
            demux.av_sync_hw_id_for_media_filter(video_filter_id),
            Some(pcr_filter_id)
        );

        demux.remove_filter(pcr_filter_id).unwrap();
        assert_eq!(demux.av_sync_hw_id_for_media_filter(video_filter_id), None);
        assert_eq!(demux.pcr_filter_id_for_av_sync_hw_id(pcr_filter_id), None);
    }

    fn first_fd_identity(snapshot: QueueDescriptorSnapshot) -> (u64, u64) {
        let (_grantors, fds, _ints, _quantum, _flags) = snapshot.into_parts();
        let metadata = fds[0].metadata().unwrap();
        (metadata.dev(), metadata.ino())
    }

    #[test]
    fn input_origin_generation_keys_are_distinct() {
        assert_ne!(TsInputOrigin::frontend(1), TsInputOrigin::frontend(2));
        assert_ne!(
            TsInputOrigin::playback_dvr(1, 1, 0),
            TsInputOrigin::playback_dvr(1, 2, 0)
        );
        assert_ne!(
            TsInputOrigin::playback_dvr(1, 1, 0),
            TsInputOrigin::playback_dvr(2, 1, 0)
        );
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
                step: SourceBoundaryStep::ValidateQueue,
                primary_error: DemuxRuntimeErrorKind::QueueMissing,
            }
        );
        assert!(!report
            .steps()
            .contains(&SourceBoundaryStep::DisconnectDownstream));
        assert_eq!(demux.filter(10).unwrap().source_relation_generation(), 1);
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
        assert!(demux
            .filter_mut(41)
            .unwrap()
            .set_source_filter_for_test(40, 1));
        assert!(!demux.queue_exists(41));

        let (report, result) = apply_filter_source_boundary_change(&mut demux, 41, None);

        assert!(result.is_err());
        assert_eq!(
            report.outcome(),
            SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue,
                primary_error: DemuxRuntimeErrorKind::QueueMissing,
            }
        );
        assert!(!report
            .steps()
            .contains(&SourceBoundaryStep::DisconnectDownstream));
        assert_eq!(
            demux.filter(41).unwrap().snapshot().source,
            FilterSource::SourceFilter {
                source_filter_id: 40,
                source_filter_generation: 1,
            }
        );
    }

    #[test]
    fn set_filter_source_non_null_allows_pes_sink_for_ts_linkcap() {
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

        let result = demux.set_filter_source_non_null(41, 40).1;

        result.unwrap();
        let sink = demux.filter(41).unwrap().snapshot();
        assert!(sink.queue_present);
        assert_eq!(
            sink.source,
            FilterSource::SourceFilter {
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

        let reset = demux.set_filter_source_non_null(51, 50).1.unwrap();

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

        let reset = demux.set_filter_source_non_null(53, 52).1.unwrap();

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
    fn source_filter_runtime_routes_raw_ts_and_preserves_downstream_boundaries() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                54,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .register_filter(open_filter_runtime_with_queue(
                55,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        for filter_id in [54, 55] {
            demux
                .configure_filter_runtime(
                    filter_id,
                    FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: filter_id == 54,
                        record_index: None,
                    },
                )
                .unwrap();
        }
        demux.set_filter_source_non_null(55, 54).1.unwrap();
        demux.start_filter_runtime(54).unwrap();
        demux.start_filter_runtime(55).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                56,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(56).unwrap();
        demux.attach_dvr_filter(56, 55).unwrap();
        demux.start_dvr_runtime(56).unwrap();

        let first = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        demux.push_ts_packet_from_origin(&first, TsInputOrigin::frontend(1));
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(54).unwrap(),
            first.to_vec()
        );
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(56).unwrap(),
            first.to_vec()
        );

        let old_source_generation = demux.filter(54).unwrap().generation();
        demux.flush_filter_runtime(54).unwrap();
        let new_source_generation = demux.filter(54).unwrap().generation();
        assert_eq!(new_source_generation, old_source_generation + 1);
        assert_eq!(demux.filter(55).unwrap().state(), FilterRuntimeState::Started);
        assert_eq!(
            demux.filter(55).unwrap().snapshot().source,
            FilterSource::SourceFilter {
                source_filter_id: 54,
                source_filter_generation: new_source_generation,
            }
        );

        let second = raw_ts_packet(0x0100, 1, &[5, 6, 7, 8]);
        demux.push_ts_packet_from_origin(&second, TsInputOrigin::frontend(1));
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(54).unwrap(),
            second.to_vec()
        );
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(56).unwrap(),
            [first.to_vec(), second.to_vec()].concat()
        );

        demux.remove_filter(54).unwrap();
        assert_eq!(demux.filter(55).unwrap().state(), FilterRuntimeState::Started);
        assert_eq!(
            demux.filter(55).unwrap().snapshot().source,
            FilterSource::DemuxInput
        );
        let third = raw_ts_packet(0x0100, 2, &[9, 10, 11, 12]);
        demux.push_ts_packet_from_origin(&third, TsInputOrigin::frontend(1));
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(56).unwrap(),
            [first.to_vec(), second.to_vec(), third.to_vec()].concat()
        );
    }

    #[test]
    fn source_reconfigure_rejects_incompatible_connected_sink_without_mutation() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [57, 58] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    1,
                    FilterOpenType::TsRaw,
                    None,
                ))
                .unwrap();
            demux
                .configure_filter_runtime(
                    filter_id,
                    FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    },
                )
                .unwrap();
        }
        demux.set_filter_source_non_null(58, 57).1.unwrap();
        let source_before = demux.filter_snapshot(57).unwrap();
        let sink_before = demux.filter_snapshot(58).unwrap();

        let (_, result) = FilterConfigureTxn::new(57).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(0x0101),
                raw: true,
                record_index: None,
            },
        );

        assert_eq!(result.unwrap_err().kind, DemuxRuntimeErrorKind::PidMismatch);
        assert_eq!(demux.filter_snapshot(57).unwrap(), source_before);
        assert_eq!(demux.filter_snapshot(58).unwrap(), sink_before);
    }

    #[test]
    fn source_reconfigure_advances_origin_and_keeps_compatible_connection() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [61, 62] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    1,
                    FilterOpenType::TsRaw,
                    None,
                ))
                .unwrap();
            demux
                .configure_filter_runtime(
                    filter_id,
                    FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    },
                )
                .unwrap();
        }
        demux.set_filter_source_non_null(62, 61).1.unwrap();
        let old_generation = demux.filter(61).unwrap().generation();

        let (_, result) = FilterConfigureTxn::new(61).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(0x0100),
                raw: false,
                record_index: None,
            },
        );

        assert!(result.is_ok());
        let new_generation = demux.filter(61).unwrap().generation();
        assert_eq!(new_generation, old_generation + 1);
        assert_eq!(
            demux.filter(62).unwrap().snapshot().source,
            FilterSource::SourceFilter {
                source_filter_id: 61,
                source_filter_generation: new_generation,
            }
        );
    }

    #[test]
    fn source_filter_connection_rejects_indirect_cycle() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [59, 60] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    1,
                    FilterOpenType::TsRaw,
                    Some(FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    }),
                ))
                .unwrap();
        }
        demux.set_filter_source_non_null(60, 59).1.unwrap();

        let error = demux.set_filter_source_non_null(59, 60).1.unwrap_err();

        assert_eq!(error.kind, DemuxRuntimeErrorKind::SelfReference);
        assert_eq!(demux.filter(59).unwrap().snapshot().source, FilterSource::DemuxInput);
    }

    #[test]
    fn source_generation_exhaustion_fails_source_without_failing_downstream() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [63, 64] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    u64::MAX,
                    FilterOpenType::TsRaw,
                    Some(FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    }),
                ))
                .unwrap();
        }
        demux.set_filter_source_non_null(64, 63).1.unwrap();

        let error = demux.flush_filter_runtime(63).unwrap_err();

        assert_eq!(error.kind, DemuxRuntimeErrorKind::GenerationExhausted);
        assert_eq!(demux.filter(63).unwrap().state(), FilterRuntimeState::Failed);
        assert_eq!(
            demux.filter(64).unwrap().state(),
            FilterRuntimeState::Configured
        );
        assert_eq!(
            demux.filter(64).unwrap().snapshot().source,
            FilterSource::SourceFilter {
                source_filter_id: 63,
                source_filter_generation: u64::MAX,
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
            demux.filter(20).unwrap().snapshot().open_type,
            FilterOpenType::TsAudio
        );
        assert_eq!(demux.filter(20).unwrap().open_kind(), PipelineOpenKind::Av);
        assert_eq!(
            demux.filter(21).unwrap().snapshot().open_type,
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

        assert_eq!(filter.snapshot().open_type, FilterOpenType::TsSection);
        assert_eq!(filter.open_kind(), PipelineOpenKind::Section);
        assert_eq!(filter.buffer_size(), 4096);
        assert!(filter.snapshot().callback_present);
    }

    #[test]
    fn open_dvr_runtime_preserves_request_boundary() {
        let dvr =
            DemuxRuntime::open_dvr_runtime(23, 1, crate::runtime::DvrKind::Playback, 8192, true);

        assert_eq!(dvr.kind(), crate::runtime::DvrKind::Playback);
        assert_eq!(dvr.buffer_size(), 8192);
        assert!(dvr.snapshot().callback_present);
        assert!(dvr.snapshot().playback_assembler_present);
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
            .filter_queue_descriptor_export_plan(24)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
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
            demux
                .filter_queue_descriptor_export_plan(25)
                .and_then(|plan| plan
                    .export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)),
            Err(QueueDescriptorQueryError::Unavailable(25))
        ));
    }

    #[test]
    fn dvr_queue_desc_exists_at_open_and_keeps_identity_after_configure() {
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

        let open_snapshot = demux
            .dvr_queue_descriptor_export_plan(26)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .expect("open DVR queue descriptor must exist");
        let open_identity = first_fd_identity(open_snapshot);

        demux.configure_dvr_runtime(26).unwrap();
        let snapshot = demux
            .dvr_queue_descriptor_export_plan(26)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .expect("configured DVR queue descriptor must exist");
        let (grantors, fds, _ints, quantum, _flags) = snapshot.into_parts();
        assert!(!grantors.is_empty());
        assert!(!fds.is_empty());
        assert!(quantum > 0);
        let metadata = fds[0].metadata().unwrap();
        assert_eq!(open_identity, (metadata.dev(), metadata.ino()));
    }

    #[test]
    fn unconfigured_playback_flush_checks_lifecycle_before_generation() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                94,
                u64::MAX,
                DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();

        let error = demux.flush_dvr_runtime(94).unwrap_err();

        assert_eq!(error.kind, DemuxRuntimeErrorKind::InvalidState);
        assert_eq!(demux.dvr(94).unwrap().state(), DvrRuntimeState::Open);
        assert_eq!(demux.dvr(94).unwrap().generation(), u64::MAX);
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
        let first = demux
            .filter_queue_descriptor_export_plan(27)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();
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
        let second = demux
            .filter_queue_descriptor_export_plan(27)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();

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
        let first = demux
            .dvr_queue_descriptor_export_plan(28)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();
        let first_identity = first_fd_identity(first);

        DvrConfigureTxn::new(28).configure(&mut demux).1.unwrap();
        let second = demux
            .dvr_queue_descriptor_export_plan(28)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();

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
                .write_playback_dvr_queue_bytes_for_test(35, &first_packet)
                .unwrap(),
            188
        );
        assert_eq!(
            demux.consume_playback_dvr_queue_for_test(35).unwrap(),
            crate::runtime::PlaybackConsumeReport::default()
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(34).unwrap(),
            Vec::<u8>::new()
        );

        demux.start_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Started);
        let first_consume = demux.consume_playback_dvr_queue_for_test(35).unwrap();
        assert_eq!(first_consume.bytes_read, 188);
        assert_eq!(first_consume.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(34).unwrap(),
            first_packet.to_vec()
        );
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(37).unwrap(),
            first_packet.to_vec()
        );

        demux.stop_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Stopped);

        let second_packet = raw_ts_packet(0x0100, 1, &[0x05, 0x06, 0x07, 0x08]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(35, &second_packet)
                .unwrap(),
            188
        );
        demux.start_dvr_runtime(35).unwrap();
        let second_consume = demux.consume_playback_dvr_queue_for_test(35).unwrap();
        assert_eq!(second_consume.bytes_read, 188);
        assert_eq!(second_consume.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(34).unwrap(),
            [first_packet.to_vec(), second_packet.to_vec()].concat()
        );
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(37).unwrap(),
            [first_packet.to_vec(), second_packet.to_vec()].concat()
        );

        demux.stop_dvr_runtime(35).unwrap();
        assert_eq!(demux.dvr(35).unwrap().state(), DvrRuntimeState::Stopped);
        let third_packet = raw_ts_packet(0x0100, 2, &[0x09, 0x0a, 0x0b, 0x0c]);
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(35, &third_packet)
                .unwrap(),
            188
        );
        demux.flush_dvr_runtime(35).unwrap();
        demux.start_dvr_runtime(35).unwrap();
        let after_flush = demux.consume_playback_dvr_queue_for_test(35).unwrap();
        assert_eq!(after_flush.bytes_read, 0);
        assert_eq!(after_flush.completed_packets, 0);
    }

    #[test]
    fn playback_dvr_consumes_when_record_is_the_only_started_output() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                80,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                80,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(80).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                81,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(81).unwrap();
        demux.attach_dvr_filter(81, 80).unwrap();
        demux.start_dvr_runtime(81).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                82,
                1,
                DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(82).unwrap();
        demux.start_dvr_runtime(82).unwrap();

        let packet = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        demux
            .write_playback_dvr_queue_bytes_for_test(82, &packet)
            .unwrap();
        let report = demux.consume_playback_dvr_queue_for_test(82).unwrap();

        assert_eq!(report.completed_packets, 1);
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(81).unwrap(),
            packet.to_vec()
        );
    }

    #[test]
    fn playback_stats_count_injection_and_flush_records_exact_boundary_drop() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                83,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                83,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(83).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                84,
                1,
                DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(84).unwrap();
        demux.start_dvr_runtime(84).unwrap();

        let valid = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        let mut malformed = valid;
        malformed[3] = 0;
        let first_input = [valid.to_vec(), malformed.to_vec()].concat();
        demux
            .write_playback_dvr_queue_bytes_for_test(84, &first_input)
            .unwrap();
        let report = demux.consume_playback_dvr_queue_for_test(84).unwrap();

        assert_eq!(report.completed_packets, 2);
        assert_eq!(report.malformed_packets, 1);
        assert_eq!(report.dropped_bytes, 188);
        assert_eq!(
            demux.dvr_snapshot(84).unwrap().playback_stats,
            PlaybackStats {
                injected_bytes: 188,
                injected_packets: 1,
                malformed_packets: 1,
                dropped_bytes: 188,
                counter_saturated: false,
            }
        );

        demux
            .write_playback_dvr_queue_bytes_for_test(84, &valid[..100])
            .unwrap();
        demux.consume_playback_dvr_queue_for_test(84).unwrap();
        demux
            .write_playback_dvr_queue_bytes_for_test(84, &valid[100..120])
            .unwrap();
        demux.flush_dvr_runtime(84).unwrap();

        let snapshot = demux.dvr_snapshot(84).unwrap();
        assert_eq!(snapshot.playback_stats, PlaybackStats::default());
        assert_eq!(
            snapshot.playback_flush_diagnostic,
            PlaybackFlushDiagnostic {
                flush_count: 1,
                total_dropped_bytes: 120,
                last_dropped_bytes: 120,
                counter_saturated: false,
            }
        );
    }

    #[test]
    fn playback_reconfigure_preserves_prefill_residual_and_stats() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                92,
                1,
                FilterOpenType::TsRaw,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                92,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: true,
                    record_index: None,
                },
            )
            .unwrap();
        demux.start_filter_runtime(92).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                93,
                1,
                DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(93).unwrap();
        demux.start_dvr_runtime(93).unwrap();

        let first = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        let second = raw_ts_packet(0x0100, 1, &[5, 6, 7, 8]);
        demux
            .write_playback_dvr_queue_bytes_for_test(
                93,
                &[first.to_vec(), second[..100].to_vec()].concat(),
            )
            .unwrap();
        assert_eq!(
            demux
                .consume_playback_dvr_queue_for_test(93)
                .unwrap()
                .completed_packets,
            1
        );
        let before = demux.dvr_snapshot(93).unwrap();
        assert_eq!(before.playback_stats.injected_packets, 1);
        demux.stop_dvr_runtime(93).unwrap();

        demux.configure_dvr_runtime(93).unwrap();
        let reconfigured = demux.dvr_snapshot(93).unwrap();
        assert_eq!(reconfigured.generation, before.generation + 1);
        assert_eq!(reconfigured.playback_stats, before.playback_stats);
        demux.start_dvr_runtime(93).unwrap();
        demux
            .write_playback_dvr_queue_bytes_for_test(93, &second[100..])
            .unwrap();
        assert_eq!(
            demux
                .consume_playback_dvr_queue_for_test(93)
                .unwrap()
                .completed_packets,
            1
        );
        assert_eq!(
            demux.dvr_snapshot(93).unwrap().playback_stats,
            PlaybackStats {
                injected_bytes: 376,
                injected_packets: 2,
                malformed_packets: 0,
                dropped_bytes: 0,
                counter_saturated: false,
            }
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(92).unwrap(),
            [first.to_vec(), second.to_vec()].concat()
        );
    }

    #[test]
    fn playback_flush_preserves_record_output_offset_and_record_fmq() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                85,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                85,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_FIRST_PACKET,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
                },
            )
            .unwrap();
        demux.start_filter_runtime(85).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                86,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(86).unwrap();
        demux.attach_dvr_filter(86, 85).unwrap();
        demux.start_dvr_runtime(86).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                87,
                1,
                DvrKind::Playback,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(87).unwrap();
        demux.start_dvr_runtime(87).unwrap();

        let first = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        demux
            .write_playback_dvr_queue_bytes_for_test(87, &first)
            .unwrap();
        let first_report = demux.consume_playback_dvr_queue_for_test(87).unwrap();
        assert!(first_report.packet_reports[0]
            .generated_events
            .iter()
            .any(|event| matches!(
                event,
                PipelineGeneratedEvent::RecordIndex { filter_id: 85, data }
                    if data.byte_number == 0
            )));

        demux.flush_dvr_runtime(87).unwrap();
        let second = raw_ts_packet(0x0100, 0, &[5, 6, 7, 8]);
        demux
            .write_playback_dvr_queue_bytes_for_test(87, &second)
            .unwrap();
        let second_report = demux.consume_playback_dvr_queue_for_test(87).unwrap();
        assert!(second_report.packet_reports[0]
            .generated_events
            .iter()
            .any(|event| matches!(
                event,
                PipelineGeneratedEvent::RecordIndex { filter_id: 85, data }
                    if data.byte_number == 188
            )));
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(86).unwrap(),
            [first.to_vec(), second.to_vec()].concat()
        );
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
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_FIRST_PACKET
                            | DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
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
        assert_eq!(
            demux.dvr(37).unwrap().record_filter_relation_generation(),
            1
        );

        demux.start_dvr_runtime(37).unwrap();
        assert_eq!(demux.dvr(37).unwrap().state(), DvrRuntimeState::Started);

        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));
        assert!(report
            .delivery_actions
            .contains(&crate::packet_pipeline::PipelineDeliveryAction::DvrMirror { dvr_id: 36 }));
        assert!(report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 36, data }
                if data.byte_number == 0
                    && data.ts_index_mask
                        == (DEMUX_TS_INDEX_FIRST_PACKET | DEMUX_TS_INDEX_PAYLOAD_UNIT_START)
        )));
        assert!(demux.flush_dvr_runtime(37).is_err());
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(37).unwrap(),
            packet.to_vec()
        );
        demux.stop_dvr_runtime(37).unwrap();
        demux.flush_dvr_runtime(37).unwrap();
        demux.start_dvr_runtime(37).unwrap();
        let second_packet = raw_ts_packet(0x0100, 1, &[0x05, 0x06, 0x07, 0x08]);
        let after_flush =
            demux.push_ts_packet_from_origin(&second_packet, TsInputOrigin::frontend(1));
        assert!(after_flush.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 36, data }
                if data.byte_number == 188
        )));
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(37).unwrap(),
            second_packet.to_vec()
        );

        demux.stop_dvr_runtime(37).unwrap();
        assert_eq!(demux.dvr(37).unwrap().state(), DvrRuntimeState::Stopped);
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(37).unwrap(),
            Vec::<u8>::new()
        );

        demux.detach_dvr_filter(37, 36).unwrap();
        demux.detach_dvr_filter(37, 36).unwrap();
        assert!(demux.dvr(37).unwrap().attached_record_filters().is_empty());
        assert_eq!(
            demux.dvr(37).unwrap().record_filter_relation_generation(),
            2
        );
    }

    #[test]
    fn record_relation_precommit_failure_keeps_old_relation_and_generation() {
        use crate::runtime::demux::RecordDvrFilterRelationCommitFault;

        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                136,
                1,
                PipelineOpenKind::Record,
                None,
            ))
            .unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                137,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.inject_next_record_filter_relation_commit_fault(
            RecordDvrFilterRelationCommitFault::RejectBeforeCommit,
        );

        let error = demux.attach_dvr_filter(137, 136).unwrap_err();
        assert_eq!(error.kind, DemuxRuntimeErrorKind::PipelineFailed);
        let dvr = demux.dvr(137).unwrap();
        assert!(dvr.attached_record_filters().is_empty());
        assert_eq!(dvr.record_filter_relation_generation(), 0);
        assert_eq!(
            dvr.record_filter_relation_state(),
            RecordDvrFilterRelationState::Healthy
        );

        demux.attach_dvr_filter(137, 136).unwrap();
        assert_eq!(
            demux.dvr(137).unwrap().record_filter_relation_generation(),
            1
        );
    }

    #[test]
    fn record_relation_commit_unknown_quarantines_only_the_relation_route() {
        use crate::runtime::demux::RecordDvrFilterRelationCommitFault;

        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                138,
                1,
                PipelineOpenKind::Record,
                None,
            ))
            .unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                139,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.inject_next_record_filter_relation_commit_fault(
            RecordDvrFilterRelationCommitFault::UnknownAfterApply,
        );

        let error = demux.attach_dvr_filter(139, 138).unwrap_err();
        assert_eq!(error.kind, DemuxRuntimeErrorKind::RelationCommitUnknown);
        let dvr = demux.dvr(139).unwrap();
        assert_eq!(
            dvr.record_filter_relation_state(),
            RecordDvrFilterRelationState::Quarantined
        );
        assert_eq!(demux.state(), DemuxRuntimeState::Open);
        assert_eq!(
            demux.detach_dvr_filter(139, 138).unwrap_err().kind,
            DemuxRuntimeErrorKind::InvalidState
        );
    }

    #[test]
    fn record_relation_generation_exhaustion_never_reuses_generation() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(DemuxRuntime::open_filter_runtime(
                140,
                1,
                PipelineOpenKind::Record,
                None,
            ))
            .unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                141,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        let dvr = demux.dvr_mut(141).unwrap();
        let mut snapshot = dvr.snapshot();
        snapshot.record_filter_relation_generation = u64::MAX;
        dvr.restore(snapshot);

        let error = demux.attach_dvr_filter(141, 140).unwrap_err();
        assert_eq!(error.kind, DemuxRuntimeErrorKind::GenerationExhausted);
        let dvr = demux.dvr(141).unwrap();
        assert!(dvr.attached_record_filters().is_empty());
        assert_eq!(dvr.record_filter_relation_generation(), u64::MAX);
        assert_eq!(
            dvr.record_filter_relation_state(),
            RecordDvrFilterRelationState::Quarantined
        );
    }

    #[test]
    fn record_dvr_writes_union_match_once_and_keeps_per_filter_indexes() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [38, 39] {
            demux
                .register_filter(open_filter_runtime_with_queue(
                    filter_id,
                    1,
                    FilterOpenType::TsRecord,
                    None,
                ))
                .unwrap();
            demux
                .configure_filter_runtime(
                    filter_id,
                    FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: false,
                        record_index: Some(RecordIndexSettings {
                            ts_index_mask: DEMUX_TS_INDEX_FIRST_PACKET,
                            sc_index_type: RECORD_SC_TYPE_NONE,
                            sc_index_mask: 0,
                        }),
                    },
                )
                .unwrap();
            demux.start_filter_runtime(filter_id).unwrap();
        }
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                40,
                1,
                crate::runtime::DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(40).unwrap();
        demux.attach_dvr_filter(40, 38).unwrap();
        demux.attach_dvr_filter(40, 39).unwrap();
        demux.start_dvr_runtime(40).unwrap();

        let packet = raw_ts_packet(0x0100, 0, &[0x01, 0x02, 0x03, 0x04]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));

        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(40).unwrap(),
            packet.to_vec()
        );
        for filter_id in [38, 39] {
            assert!(report.generated_events.iter().any(|event| matches!(
                event,
                PipelineGeneratedEvent::RecordIndex {
                    filter_id: event_filter_id,
                    data,
                } if *event_filter_id == filter_id && data.byte_number == 0
            )));
        }
    }

    #[test]
    fn record_index_skips_counter_collision_but_keeps_committed_byte_position() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                88,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                88,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_FIRST_PACKET
                            | DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
                },
            )
            .unwrap();
        demux.start_filter_runtime(88).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                89,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(89).unwrap();
        demux.attach_dvr_filter(89, 88).unwrap();
        demux.start_dvr_runtime(89).unwrap();

        let first = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        let first_report =
            demux.push_ts_packet_from_origin(&first, TsInputOrigin::frontend(1));
        assert!(first_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 88, data }
                if data.byte_number == 0
        )));

        let mut collision = first;
        collision[20] ^= 0x01;
        let collision_report =
            demux.push_ts_packet_from_origin(&collision, TsInputOrigin::frontend(1));
        assert!(collision_report.generated_events.iter().all(|event| !matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 88, .. }
        )));

        let third = raw_ts_packet(0x0100, 1, &[5, 6, 7, 8]);
        let third_report =
            demux.push_ts_packet_from_origin(&third, TsInputOrigin::frontend(1));
        assert!(third_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 88, data }
                if data.byte_number == 376
        )));
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(89).unwrap(),
            [first.to_vec(), collision.to_vec(), third.to_vec()].concat()
        );
    }

    #[test]
    fn record_index_parses_adaptation_only_and_keyless_scrambled_commits() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(open_filter_runtime_with_queue(
                90,
                1,
                FilterOpenType::TsRecord,
                None,
            ))
            .unwrap();
        demux
            .configure_filter_runtime(
                90,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_PCR
                            | DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
                },
            )
            .unwrap();
        demux.start_filter_runtime(90).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                91,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(91).unwrap();
        demux.attach_dvr_filter(91, 90).unwrap();
        demux.start_dvr_runtime(91).unwrap();

        let adaptation_only = pcr_packet(0x0100, 0, 90_000);
        let adaptation_report =
            demux.push_ts_packet_from_origin(&adaptation_only, TsInputOrigin::frontend(1));
        assert!(adaptation_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 90, data }
                if data.byte_number == 0 && data.ts_index_mask == DEMUX_TS_INDEX_PCR
        )));

        let mut scrambled = raw_ts_packet(0x0100, 0, &[1, 2, 3, 4]);
        scrambled[3] = 0x90;
        let scrambled_report =
            demux.push_ts_packet_from_origin(&scrambled, TsInputOrigin::frontend(1));
        assert!(scrambled_report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler));
        assert!(scrambled_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 90, data }
                if data.byte_number == 188
                    && data.ts_index_mask == DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED
        )));
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(91).unwrap(),
            [adaptation_only.to_vec(), scrambled.to_vec()].concat()
        );
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
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_FIRST_PACKET,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
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

        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));

        assert!(report.diagnostics.contains(
            &crate::packet_pipeline::PipelineDiagnostic::RecordDvrMirrorOverflow {
                pid: packet_pid(0x0100),
                source_filter_id: 36,
                dvr_id: 37,
            }
        ));
        assert!(demux.dvr(37).unwrap().snapshot().pending_overflow);
        assert!(report.generated_events.iter().all(|event| !matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { .. }
        )));
        demux.stop_dvr_runtime(37).unwrap();
        demux.flush_dvr_runtime(37).unwrap();
        assert!(!demux.dvr(37).unwrap().snapshot().pending_overflow);
    }

    #[test]
    fn demux_runtime_malformed_packet_reports_drop_without_accepting_packet() {
        let mut demux = DemuxRuntime::new(1, 1);
        let malformed = [0xffu8; 187];

        let report = demux.push_ts_packet_from_origin(&malformed, TsInputOrigin::frontend(1));

        assert_eq!(report.accepted_packets, 0);
        assert_eq!(report.dropped_packets, 1);
        assert_eq!(report.malformed_packets, 1);
        assert_eq!(
            demux
                .pipeline_diagnostic_counters()
                .malformed_ts_packets,
            1
        );
        assert!(report
            .drop_reasons
            .contains(&crate::packet_pipeline::PipelineDropReason::MalformedPacket));
        assert!(report.diagnostics.contains(
            &crate::packet_pipeline::PipelineDiagnostic::MalformedTsPacket {
                reason: TsPacketValidationError::WrongLength,
            }
        ));
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
        assert_eq!(
            demux.dvr(42).unwrap().snapshot().status_check_interval_ms,
            250
        );
        assert_eq!(demux.dvr(42).unwrap().state(), DvrRuntimeState::Open);

        demux.configure_dvr_runtime(42).unwrap();
        demux.start_dvr_runtime(42).unwrap();
        demux.set_dvr_status_check_interval(42, 500).unwrap();
        assert_eq!(
            demux.dvr(42).unwrap().snapshot().status_check_interval_ms,
            500
        );
        assert_eq!(demux.dvr(42).unwrap().state(), DvrRuntimeState::Started);

        demux.stop_dvr_runtime(42).unwrap();
        demux.set_dvr_status_check_interval(42, 750).unwrap();
        assert_eq!(
            demux.dvr(42).unwrap().snapshot().status_check_interval_ms,
            750
        );
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
            .configure_settings(0b1111, 0, 188, DvrDataFormat::Ts, 188);
        let record_settings = demux.dvr_snapshot(420).unwrap();
        assert_eq!(record_settings.data_format, Some(DvrDataFormat::Ts));
        assert_eq!(record_settings.packet_size, Some(188));
        assert_eq!(demux.dvr_status_event(420).unwrap(), None);
        demux
            .dvr_mut(420)
            .unwrap()
            .configure_settings(0b1111, 1, 187, DvrDataFormat::Ts, 188);
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
            .configure_settings(0b1111, 47, 141, DvrDataFormat::Ts, 188);
        let playback_settings = demux.dvr_snapshot(421).unwrap();
        assert_eq!(playback_settings.data_format, Some(DvrDataFormat::Ts));
        assert_eq!(playback_settings.packet_size, Some(188));
        assert_eq!(
            demux.dvr_status_event(421).unwrap(),
            Some(crate::runtime::DvrStatusEvent::PlaybackSpaceAlmostEmpty)
        );
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(421, &packet)
                .unwrap(),
            188
        );
        assert_eq!(
            demux.dvr_status_event(421).unwrap(),
            Some(crate::runtime::DvrStatusEvent::PlaybackSpaceFull)
        );
    }

    #[test]
    fn masked_record_event_does_not_fall_through_to_watermark_in_same_evaluation() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                422,
                1,
                crate::runtime::DvrKind::Record,
                188,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(422).unwrap();
        demux
            .dvr_mut(422)
            .unwrap()
            .configure_settings(0b0010, 1, 187, DvrDataFormat::Ts, 188);
        demux.dvr_mut(422).unwrap().mark_pending_overflow();

        assert_eq!(demux.dvr_status_event(422).unwrap(), None);
        assert_eq!(
            demux.dvr_status_event(422).unwrap(),
            Some(crate::runtime::DvrStatusEvent::RecordLowWater)
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
            demux
                .read_record_dvr_queue_bytes_for_test(43)
                .unwrap_err()
                .kind,
            crate::runtime::DemuxRuntimeErrorKind::InvalidState
        );
        demux.configure_dvr_runtime(43).unwrap();
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(43).unwrap(),
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
            demux
                .read_record_dvr_queue_bytes_for_test(44)
                .unwrap_err()
                .kind,
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
                .write_playback_dvr_queue_bytes_for_test(45, &packet)
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
                .write_playback_dvr_queue_bytes_for_test(46, &packet)
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
            demux
                .write_playback_dvr_queue_bytes_for_test(47, &packet)
                .unwrap(),
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
                .write_playback_dvr_queue_bytes_for_test(49, &packet[..100])
                .unwrap(),
            100
        );
        let first = demux.consume_playback_dvr_queue_for_test(49).unwrap();
        assert_eq!(first.completed_packets, 0);
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(48).unwrap(),
            Vec::<u8>::new()
        );

        demux.stop_dvr_runtime(49).unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(49, &packet[100..])
                .unwrap(),
            88
        );
        demux.start_dvr_runtime(49).unwrap();
        let second = demux.consume_playback_dvr_queue_for_test(49).unwrap();
        assert_eq!(second.completed_packets, 1);
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(48).unwrap(),
            packet.to_vec()
        );

        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(49, &packet[..100])
                .unwrap(),
            100
        );
        let third = demux.consume_playback_dvr_queue_for_test(49).unwrap();
        assert_eq!(third.completed_packets, 0);
        demux.flush_dvr_runtime(49).unwrap();
        assert_eq!(
            demux
                .write_playback_dvr_queue_bytes_for_test(49, &packet[100..])
                .unwrap(),
            88
        );
        let after_flush = demux.consume_playback_dvr_queue_for_test(49).unwrap();
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
        let before_restore = demux
            .filter_queue_descriptor_export_plan(29)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();
        let before_identity = first_fd_identity(before_restore);
        let snapshot = demux.snapshot();

        demux.restore(snapshot).unwrap();
        let after_restore = demux
            .filter_queue_descriptor_export_plan(29)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .unwrap();

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
            Some(FilterConfigureOutcome::Failed {
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
        assert_eq!(demux.filter(12).unwrap().snapshot().tpid, Some(101));
    }

    #[test]
    fn filter_reconfigure_preserves_compatible_source_binding() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [60, 61] {
            demux
                .register_filter(DemuxRuntime::open_filter_runtime(
                    filter_id,
                    1,
                    PipelineOpenKind::Raw,
                    Some(FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    }),
                ))
                .unwrap();
            demux.create_filter_queue(filter_id).unwrap();
        }
        demux.set_filter_source_non_null(61, 60).1.unwrap();

        let (_, result) = FilterConfigureTxn::new(61).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(0x0100),
                raw: false,
                record_index: None,
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            demux.filter(61).unwrap().snapshot().source,
            FilterSource::SourceFilter {
                source_filter_id: 60,
                source_filter_generation: 1,
            }
        );
    }

    #[test]
    fn filter_reconfigure_rejects_incompatible_source_pid_without_mutation() {
        let mut demux = DemuxRuntime::new(1, 1);
        for filter_id in [62, 63] {
            demux
                .register_filter(DemuxRuntime::open_filter_runtime(
                    filter_id,
                    1,
                    PipelineOpenKind::Raw,
                    Some(FilterPipelineConfig {
                        tpid: Some(0x0100),
                        raw: true,
                        record_index: None,
                    }),
                ))
                .unwrap();
            demux.create_filter_queue(filter_id).unwrap();
        }
        demux.set_filter_source_non_null(63, 62).1.unwrap();
        let before = demux.filter(63).unwrap().snapshot();

        let (txn, result) = FilterConfigureTxn::new(63).configure(
            &mut demux,
            PipelineOpenKind::Raw,
            FilterPipelineConfig {
                tpid: Some(0x0101),
                raw: false,
                record_index: None,
            },
        );

        assert!(result.is_err());
        assert_eq!(
            txn.outcome(),
            Some(FilterConfigureOutcome::Failed {
                failed_step: FilterConfigureStep::ValidateSettings,
            })
        );
        assert_eq!(demux.filter(63).unwrap().snapshot(), before);
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
            Some(FilterConfigureOutcome::Failed {
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
    fn av_stream_type_hint_keeps_delivered_allocation_until_release() {
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
        assert_eq!(
            demux.filter(16).unwrap().snapshot().av_stream_type_hint,
            Some(hint)
        );

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
        assert_eq!(
            demux.filter(16).unwrap().snapshot().av_stream_type_hint,
            None
        );
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
        assert!(!demux.filter(18).unwrap().snapshot().queue_present);
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
            demux.filter(18).unwrap().snapshot().av_stream_type_hint,
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

        let error = demux
            .release_filter_av_handle(19, AvHandleReleaseDescriptor::Empty, 0)
            .unwrap_err();

        assert_eq!(
            error.kind,
            crate::runtime::DemuxRuntimeErrorKind::AvBackingFailure
        );
    }

    #[test]
    fn av_configure_stream_type_keeps_delivered_slots_and_backing_exported() {
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

        assert_eq!(demux.filter_av_active_slot_count_for_test(19), Some(1));
        assert_eq!(
            demux
                .release_filter_av_handle(19, AvHandleReleaseDescriptor::Empty, data_id.0)
                .unwrap(),
            AvHandleReleaseOutcome::SlotReleased { data_id }
        );
        assert!(matches!(
            demux.allocate_filter_av_payload_for_test(19, 188).unwrap(),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }

    #[test]
    fn av_flush_keeps_delivered_slots_until_explicit_release() {
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

        assert_eq!(demux.filter_av_active_slot_count_for_test(20), Some(1));
        assert_eq!(
            demux
                .release_filter_av_handle(20, AvHandleReleaseDescriptor::Empty, data_id.0)
                .unwrap(),
            AvHandleReleaseOutcome::SlotReleased { data_id }
        );
        assert!(matches!(
            demux.allocate_filter_av_payload_for_test(20, 188).unwrap(),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }

    #[test]
    fn closed_av_filter_release_accepts_active_token_once() {
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
            demux
                .release_filter_av_handle(21, AvHandleReleaseDescriptor::Empty, 999)
                .unwrap(),
            AvHandleReleaseOutcome::UnknownDataId
        );
        assert_eq!(
            demux
                .release_filter_av_handle(
                    21,
                    AvHandleReleaseDescriptor::Empty,
                    stale_data_id.0,
                )
                .unwrap(),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: stale_data_id
            }
        );
        assert_eq!(
            demux
                .release_filter_av_handle(
                    21,
                    AvHandleReleaseDescriptor::Empty,
                    stale_data_id.0,
                )
                .unwrap(),
            AvHandleReleaseOutcome::UnknownDataId
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
        let hints = demux.filter(17).unwrap().snapshot().delay_hints;
        assert_eq!(hints.time_delay_ms, Some(10));
        assert_eq!(hints.data_size_delay_bytes, Some(188));

        demux
            .set_filter_delay_hint(17, FilterDelayHint::TimeDelayMs(0))
            .unwrap();
        demux
            .set_filter_delay_hint(17, FilterDelayHint::DataSizeDelayBytes(0))
            .unwrap();
        let hints = demux.filter(17).unwrap().snapshot().delay_hints;
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
            demux.filter_delivery_readiness_for_test(30).unwrap(),
            FilterDelayReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness_for_test(30).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.drain_filter_queue_for_delivery_for_test(30).unwrap(),
            vec![vec![1, 2, 3]]
        );

        demux
            .enqueue_filter_queue_payload(30, vec![4, 5, 6])
            .unwrap();
        assert_eq!(
            demux.filter_delivery_readiness_for_test(30).unwrap(),
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
            demux.filter_delivery_readiness_for_test(31).unwrap(),
            FilterDelayReadiness::WaitingForDataSize
        );

        demux.enqueue_filter_queue_payload(31, vec![4, 5]).unwrap();
        assert_eq!(
            demux.filter_delivery_readiness_for_test(31).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(31).unwrap(),
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
            demux.filter_delivery_readiness_for_test(32).unwrap(),
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
            demux.filter_delivery_readiness_for_test(33).unwrap(),
            FilterDelayReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness_for_test(33).unwrap(),
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
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));

        assert_eq!(report.accepted_packets, 1);
        assert!(report
            .generated_events
            .contains(&packet_pipeline::PipelineGeneratedEvent::DataReady { filter_id: 34 }));
        assert_eq!(
            demux.filter_delivery_readiness_for_test(34).unwrap(),
            FilterDelayReadiness::Ready
        );
        assert_eq!(
            demux.snapshot_filter_queue_bytes_for_test(34).unwrap(),
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
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::frontend(1));
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::frontend(1),
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
            TsInputOrigin::frontend(1),
            packet_pid(0x0100),
            22
        )));
    }

    #[test]
    fn explicit_pes_stream_id_drops_other_stream_ids_before_delivery() {
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
            .configure_filter_runtime_with_pes_stream_id(
                23,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: None,
                },
                Some(0xe1),
            )
            .unwrap();
        demux.start_filter_runtime(23).unwrap();

        let e0_pes = pes_start_packet(
            0x0100,
            0,
            &[0x00, 0x00, 0x01, 0xe0, 0x00, 0x04, 0x80, 0x00, 0x00, 0xde],
        );
        let report = demux.push_ts_packet_from_origin(&e0_pes, TsInputOrigin::frontend(1));

        assert!(!report.generated_events.iter().any(|event| matches!(
            event,
            packet_pipeline::PipelineGeneratedEvent::PesPacketReady { filter_id: 23, .. }
        )));
        assert!(demux
            .snapshot_filter_queue_bytes_for_test(23)
            .unwrap()
            .is_empty());

        let e1_pes = pes_start_packet(
            0x0100,
            1,
            &[0x00, 0x00, 0x01, 0xe1, 0x00, 0x04, 0x80, 0x00, 0x00, 0xad],
        );
        let report = demux.push_ts_packet_from_origin(&e1_pes, TsInputOrigin::frontend(1));
        assert!(report.generated_events.iter().any(|event| matches!(
            event,
            packet_pipeline::PipelineGeneratedEvent::PesPacketReady {
                filter_id: 23,
                packet,
                ..
            } if packet.stream_id == 0xe1
        )));
        assert!(!demux
            .snapshot_filter_queue_bytes_for_test(23)
            .unwrap()
            .is_empty());
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
        let report = demux.push_ts_packet_from_origin(&partial_pes, TsInputOrigin::frontend(1));
        assert_eq!(report.accepted_packets, 1);
        assert!(demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::frontend(1),
            packet_pid(0x0100),
            23
        )));
        assert!(demux.queue_exists(23));

        let removed = demux.remove_filter(23).unwrap();

        assert_eq!(removed.state, FilterRuntimeState::Started);
        assert!(demux.filter(23).is_none());
        assert!(!demux.queue_exists(23));
        assert!(!demux.pipeline().pes_assemblers.contains_key(&(
            TsInputOrigin::frontend(1),
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
    fn generation_boundary_overflow_quarantines_demux() {
        let mut demux = DemuxRuntime::new(1, u64::MAX);
        let result = demux.apply_stream_boundary(PipelineBoundaryReason::TuneStart);
        assert!(result.is_err());
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
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
            .insert((TsInputOrigin::frontend(1), pid), u64::MAX);

        let packet = raw_ts_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0x02]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));

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
            .insert((TsInputOrigin::frontend(1), pid), u64::MAX);

        let packet = pes_start_packet(0x0100, 0, &[0x00, 0x00, 0x01, 0xe0, 0x00]);
        let report = demux.push_ts_packet_from_origin(&packet, TsInputOrigin::frontend(1));

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
            .apply_stream_boundary(PipelineBoundaryReason::TuneStart)
            .unwrap();
        assert_eq!(report.next_generation, DemuxStreamGeneration(8));
        assert_eq!(demux.generation(), 8);
    }
}
