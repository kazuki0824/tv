//! 本番変換結果として使う型付きdemux filter設定。
//!
//! AIDL生成union / enumは `tuner_hal2/binder_adapter/src/aidl_filter_config.rs` で
//! これらの型へ直接変換する。Debug文字列や文字列field list表現をruntime正本にしない。

use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterOpenType {
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
        match self {
            Self::TsAudio | Self::TsVideo => true,
            _ => false,
        }
    }

    pub const fn pipeline_open_kind(self) -> PipelineOpenKind {
        match self {
            Self::TsRaw | Self::TsPcr => PipelineOpenKind::Raw,
            Self::TsAudio | Self::TsVideo => PipelineOpenKind::Av,
            Self::TsSection => PipelineOpenKind::Section,
            Self::TsPes => PipelineOpenKind::Pes,
            Self::TsRecord => PipelineOpenKind::Record,
        }
    }

    pub const fn from_pipeline_open_kind(open_kind: PipelineOpenKind) -> Option<Self> {
        match open_kind {
            PipelineOpenKind::Raw => Some(Self::TsRaw),
            PipelineOpenKind::Av => None,
            PipelineOpenKind::Section => Some(Self::TsSection),
            PipelineOpenKind::Pes => Some(Self::TsPes),
            PipelineOpenKind::Record => Some(Self::TsRecord),
            PipelineOpenKind::Other => None,
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
pub struct ConfigInputPid(i32);

impl ConfigInputPid {
    pub fn validate_tpid(pid: i32) -> Option<Self> {
        if (0..=0x1fff).contains(&pid) {
            Some(Self(pid))
        } else {
            None
        }
    }

    pub const fn raw(self) -> i32 {
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
    pub fn validated_tpid(&self) -> Option<ConfigInputPid> {
        ConfigInputPid::validate_tpid(self.tpid)
    }

    pub fn pipeline_config(&self) -> FilterPipelineConfig {
        let tpid = self.validated_tpid();
        FilterPipelineConfig {
            tpid: tpid.map(ConfigInputPid::raw),
            raw: match &self.kind {
                FilterConfigKind::TsSection { raw, .. } => *raw,
                FilterConfigKind::TsPes(settings) => settings.raw,
                _ => false,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenFilterRequest {
    pub open_type: FilterOpenType,
    pub buffer_size: i32,
    pub callback_present: bool,
}
