//! 本番変換結果として使う型付きdemux filter設定。
//!
//! AIDL生成union / enumは `tuner_hal2/binder_adapter/src/aidl_filter_config.rs` で
//! これらの型へ直接変換する。Debug文字列や文字列field list表現をruntime正本にしない。

use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterOpenType {
    TsRaw,
    TsAudio,
    TsVideo,
    TsSection,
    TsPes,
    TsRecord,
}

impl FilterOpenType {
    pub const fn pipeline_open_kind(self) -> PipelineOpenKind {
        match self {
            Self::TsRaw => PipelineOpenKind::Raw,
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
    pub fn pipeline_config(&self) -> FilterPipelineConfig {
        FilterPipelineConfig {
            tpid: Some(self.tpid),
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
