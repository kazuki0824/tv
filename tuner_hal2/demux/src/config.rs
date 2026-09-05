//! 本番変換結果として使う型付きdemux filter設定。
//!
//! AIDL生成union / enumは `tuner_hal2/binder_adapter/src/aidl_filter_config.rs` で
//! これらの型へ直接変換する。Debug文字列や文字列field list表現をruntime正本にしない。

use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};
use maleicacid_tuner_hal2_common::TransportStreamPid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterOpenType {
    TsUndefined,
    TsRaw,
    TsPcr,
    TsAudio,
    TsVideo,
    TsSection,
    TsPes,
    TsRecord,
}

impl FilterOpenType {
    pub const fn is_media_filter(self) -> bool {
        matches!(self, Self::TsAudio | Self::TsVideo)
    }

    pub const fn has_filter_fmq(self) -> bool {
        matches!(
            self,
            Self::TsRaw | Self::TsSection | Self::TsPes | Self::TsRecord
        )
    }

    pub const fn uses_filter_fmq_for_payload(self) -> bool {
        matches!(self, Self::TsRaw | Self::TsSection | Self::TsPes)
    }

    pub const fn pipeline_open_kind(self) -> PipelineOpenKind {
        match self {
            Self::TsUndefined => PipelineOpenKind::Raw,
            Self::TsRaw => PipelineOpenKind::Raw,
            Self::TsPcr => PipelineOpenKind::Pcr,
            Self::TsAudio | Self::TsVideo => PipelineOpenKind::Av,
            Self::TsSection => PipelineOpenKind::Section,
            Self::TsPes => PipelineOpenKind::Pes,
            Self::TsRecord => PipelineOpenKind::Record,
        }
    }

    pub const fn from_pipeline_open_kind(open_kind: PipelineOpenKind) -> Option<Self> {
        match open_kind {
            PipelineOpenKind::Raw => Some(Self::TsRaw),
            PipelineOpenKind::Pcr => Some(Self::TsPcr),
            PipelineOpenKind::Av => None,
            PipelineOpenKind::Section => Some(Self::TsSection),
            PipelineOpenKind::Pes => Some(Self::TsPes),
            PipelineOpenKind::Record => Some(Self::TsRecord),
            PipelineOpenKind::Other => Some(Self::TsUndefined),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SectionConditionKind {
    SectionBits,
    TableInfo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SectionCondition {
    pub kind: SectionConditionKind,
    pub filter: Vec<u8>,
    pub mask: Vec<u8>,
    pub mode: Vec<u8>,
    pub table_id: Option<i32>,
    pub version: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SectionRuntimeConfig {
    pub check_crc: bool,
    pub repeat: bool,
    pub length_field_bits: i32,
    pub condition: SectionCondition,
}

impl SectionRuntimeConfig {
    #[cfg(test)]
    pub(crate) fn match_all_repeat() -> Self {
        Self {
            check_crc: false,
            repeat: true,
            length_field_bits: 12,
            condition: SectionCondition {
                kind: SectionConditionKind::SectionBits,
                filter: Vec::new(),
                mask: Vec::new(),
                mode: Vec::new(),
                table_id: None,
                version: None,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordIndexSettings {
    pub ts_index_mask: i32,
    pub sc_index_type: i32,
    pub sc_index_mask: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PesSettings {
    pub stream_id: i32,
    pub raw: bool,
}

pub const PES_STREAM_ID_WILDCARD: i32 = 0xffff;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvSettings {
    pub is_passthrough: bool,
    pub is_secure_memory: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvStreamKind {
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvStreamTypeConfig {
    pub kind: AvStreamKind,
    pub stream_type: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FilterDelayHints {
    pub time_delay_ms: Option<u64>,
    pub data_size_delay_bytes: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDelayReadiness {
    Ready,
    WaitingForTime,
    WaitingForDataSize,
}

impl FilterDelayHints {
    pub fn delivery_readiness(
        self,
        elapsed_ms_since_queue_armed: u64,
        queued_bytes: usize,
    ) -> FilterDelayReadiness {
        let time_delay_ms = self.time_delay_ms.unwrap_or(0);
        let data_size_delay_bytes = self.data_size_delay_bytes.unwrap_or(0);
        if time_delay_ms == 0 && data_size_delay_bytes == 0 {
            return FilterDelayReadiness::Ready;
        }
        if time_delay_ms > 0 && elapsed_ms_since_queue_armed >= time_delay_ms {
            return FilterDelayReadiness::Ready;
        }
        if data_size_delay_bytes > 0 && queued_bytes >= data_size_delay_bytes {
            return FilterDelayReadiness::Ready;
        }
        if time_delay_ms > 0 {
            FilterDelayReadiness::WaitingForTime
        } else {
            FilterDelayReadiness::WaitingForDataSize
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDelayHint {
    TimeDelayMs(u64),
    DataSizeDelayBytes(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ConfigInputPid(TransportStreamPid);

impl ConfigInputPid {
    pub(crate) fn validate_tpid(pid: i32) -> Option<Self> {
        TransportStreamPid::validate_i32(pid).ok().map(Self)
    }

    pub(crate) const fn raw(self) -> i32 {
        self.0.to_i32_for_aidl_boundary()
    }

    pub(crate) const fn transport_stream_pid(self) -> TransportStreamPid {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilterConfigKind {
    TsRaw,
    TsSection {
        check_crc: bool,
        repeat: bool,
        raw: bool,
        length_field_bits: i32,
        condition: SectionCondition,
    },
    TsAv(AvSettings),
    TsPes(PesSettings),
    TsRecord(RecordIndexSettings),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterConfig {
    pub open_type: FilterOpenType,
    pub tpid: i32,
    pub kind: FilterConfigKind,
}

impl FilterConfig {
    pub(crate) fn validated_tpid(&self) -> Option<ConfigInputPid> {
        ConfigInputPid::validate_tpid(self.tpid)
    }

    pub(crate) fn pipeline_config(&self) -> FilterPipelineConfig {
        let tpid = self.validated_tpid();
        FilterPipelineConfig {
            tpid: tpid.map(ConfigInputPid::raw),
            raw: match &self.kind {
                FilterConfigKind::TsSection { raw, .. } => *raw,
                FilterConfigKind::TsPes(settings) => settings.raw,
                _ => false,
            },
            record_index: match &self.kind {
                FilterConfigKind::TsRecord(settings) => Some(settings.clone()),
                _ => None,
            },
        }
    }

    pub(crate) fn section_runtime_config(&self) -> Option<SectionRuntimeConfig> {
        match &self.kind {
            FilterConfigKind::TsSection {
                check_crc,
                repeat,
                length_field_bits,
                condition,
                ..
            } => Some(SectionRuntimeConfig {
                check_crc: *check_crc,
                repeat: *repeat,
                length_field_bits: *length_field_bits,
                condition: condition.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenFilterRequest {
    pub open_type: FilterOpenType,
    pub buffer_size: i32,
    pub callback_present: bool,
}

#[cfg(test)]
mod tests {
    use super::{FilterOpenType, OpenFilterRequest, RecordIndexSettings};
    use crate::packet_pipeline::FilterPipelineConfig;
    use crate::{
        DemuxRuntime, DvrKind, PipelineGeneratedEvent, QueueDescriptorQueryError, TsInputOrigin,
        DEMUX_TS_INDEX_PAYLOAD_UNIT_START, RECORD_SC_TYPE_NONE,
    };

    fn record_packet(pid: u16, continuity_counter: u8) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        packet
    }

    #[test]
    fn record_filter_owns_filter_fmq_without_using_it_for_payload() {
        assert!(FilterOpenType::TsRecord.has_filter_fmq());
        assert!(!FilterOpenType::TsRecord.uses_filter_fmq_for_payload());

        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsRecord,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter_from_open_request(1, &request)
            .expect("record filter open must create its Filter FMQ");

        assert!(demux.queue_exists(1));
        let descriptor = demux
            .filter_queue_descriptor_export_plan(1)
            .and_then(|plan| {
                plan.export_descriptor()
                    .map_err(QueueDescriptorQueryError::Runtime)
            })
            .expect("record filter getQueueDesc path must export a valid FMQ descriptor");
        let (grantors, fds, _ints, quantum, _flags) = descriptor.into_parts();
        assert!(!grantors.is_empty());
        assert!(!fds.is_empty());
        assert!(quantum > 0);
    }

    #[test]
    fn record_payload_and_byte_number_follow_record_dvr_not_filter_fmq() {
        let mut demux = DemuxRuntime::new(1, 1);
        let request = OpenFilterRequest {
            open_type: FilterOpenType::TsRecord,
            buffer_size: 4096,
            callback_present: true,
        };
        demux
            .register_filter_from_open_request(1, &request)
            .unwrap();
        demux
            .configure_filter_runtime(
                1,
                FilterPipelineConfig {
                    tpid: Some(0x0100),
                    raw: false,
                    record_index: Some(RecordIndexSettings {
                        ts_index_mask: DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
                        sc_index_type: RECORD_SC_TYPE_NONE,
                        sc_index_mask: 0,
                    }),
                },
            )
            .unwrap();
        demux.start_filter_runtime(1).unwrap();
        demux
            .register_dvr(DemuxRuntime::open_dvr_runtime(
                2,
                1,
                DvrKind::Record,
                8192,
                true,
            ))
            .unwrap();
        demux.configure_dvr_runtime(2).unwrap();
        demux.attach_dvr_filter(2, 1).unwrap();
        demux.start_dvr_runtime(2).unwrap();

        let first = record_packet(0x0100, 0);
        let first_report = demux.push_ts_packet_from_origin(&first, TsInputOrigin::frontend(1));
        assert!(first_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 1, data }
                if data.byte_number == 0
        )));
        assert!(first_report.generated_events.iter().all(|event| !matches!(
            event,
            PipelineGeneratedEvent::FilterStatus { filter_id: 1, .. }
        )));
        assert!(demux
            .snapshot_filter_queue_bytes_for_test(1)
            .expect("record Filter FMQ mirror must exist")
            .is_empty());

        let second = record_packet(0x0100, 1);
        let second_report = demux.push_ts_packet_from_origin(&second, TsInputOrigin::frontend(1));
        assert!(second_report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::RecordIndex { filter_id: 1, data }
                if data.byte_number == 188
        )));
        assert!(second_report.generated_events.iter().all(|event| !matches!(
            event,
            PipelineGeneratedEvent::FilterStatus { filter_id: 1, .. }
        )));
        assert!(demux
            .snapshot_filter_queue_bytes_for_test(1)
            .expect("record Filter FMQ mirror must exist")
            .is_empty());
        assert_eq!(
            demux.read_record_dvr_queue_bytes_for_test(2).unwrap(),
            [first.to_vec(), second.to_vec()].concat()
        );
    }
}
