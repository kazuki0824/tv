//! AIDL型から内部の型付きfilter設定へ直接変換する層。
//!
//! Debug文字列、文字列field list、欠落値の寛容な既定値補完には依存しない。
//! filter open / configure 検証の本番正本である。

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxFilterMainType::DemuxFilterMainType, DemuxFilterScIndexMask::DemuxFilterScIndexMask,
    DemuxFilterSectionBits::DemuxFilterSectionBits,
    DemuxFilterSectionSettings::DemuxFilterSectionSettings,
    DemuxFilterSectionSettingsCondition::DemuxFilterSectionSettingsCondition,
    DemuxFilterSectionSettingsConditionTableInfo::DemuxFilterSectionSettingsConditionTableInfo,
    DemuxFilterSettings::DemuxFilterSettings, DemuxFilterSubType::DemuxFilterSubType,
    DemuxFilterType::DemuxFilterType,
    DemuxTsFilterSettingsFilterSettings::DemuxTsFilterSettingsFilterSettings,
    DemuxTsFilterType::DemuxTsFilterType,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_demux::config::{
    AvSettings, FilterConfig, FilterConfigKind, FilterOpenType, OpenFilterRequest, PesSettings,
    RecordIndexSettings, SectionCondition, SectionConditionKind,
};
use maleicacid_tuner_hal2_demux::parser::record_index::{
    AVC_SC_B_SLICE, AVC_SC_I_SLICE, AVC_SC_P_SLICE, AVC_SC_SI_SLICE, AVC_SC_SP_SLICE,
    DEMUX_TS_INDEX_ADAPTATION_EXTENSION, DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
    DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED, DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED,
    DEMUX_TS_INDEX_DISCONTINUITY, DEMUX_TS_INDEX_FIRST_PACKET, DEMUX_TS_INDEX_OPCR,
    DEMUX_TS_INDEX_PAYLOAD_UNIT_START, DEMUX_TS_INDEX_PCR, DEMUX_TS_INDEX_PRIORITY,
    DEMUX_TS_INDEX_PRIVATE_DATA, DEMUX_TS_INDEX_RANDOM_ACCESS, DEMUX_TS_INDEX_SPLICING_POINT,
    HEVC_SC_AUD, HEVC_SC_BLA_N_LP, HEVC_SC_BLA_W_LP, HEVC_SC_BLA_W_RADL, HEVC_SC_IDR_N_LP,
    HEVC_SC_IDR_W_RADL, HEVC_SC_SPS, HEVC_SC_TRAIL_CRA, RECORD_SC_TYPE_NONE, RECORD_SC_TYPE_SC,
    RECORD_SC_TYPE_SC_AVC, RECORD_SC_TYPE_SC_HEVC, RECORD_SC_TYPE_SC_VVC, VVC_SC_AUD, VVC_SC_CRA,
    VVC_SC_GDR, VVC_SC_IDR_N_LP, VVC_SC_IDR_W_RADL, VVC_SC_SPS, VVC_SC_VPS,
};
use maleicacid_tuner_hal2_demux::parser::sections::normalize_length_field_bits;

const PES_STREAM_ID_WILDCARD: i32 = -1;
const MAX_SECTION_FILTER_BYTES: usize = 16;

fn invalid(detail: &'static str) -> HalError {
    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, detail)
}

pub fn filter_main_type_supported(main_type: DemuxFilterMainType) -> bool {
    main_type == DemuxFilterMainType::TS
}

pub fn filter_open_type(filter_type: &DemuxFilterType) -> Result<FilterOpenType, HalError> {
    if !filter_main_type_supported(filter_type.mainType) {
        return Err(HalError::Unsupported(
            "filter main type is outside the TS-only tuner_hal2 profile",
        ));
    }
    match &filter_type.subType {
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::TS) => Ok(FilterOpenType::TsRaw),
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::AUDIO) => Ok(FilterOpenType::TsAudio),
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::VIDEO) => Ok(FilterOpenType::TsVideo),
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::SECTION) => {
            Ok(FilterOpenType::TsSection)
        }
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::PES) => Ok(FilterOpenType::TsPes),
        DemuxFilterSubType::TsFilterType(DemuxTsFilterType::RECORD) => Ok(FilterOpenType::TsRecord),
        _ => Err(HalError::Unsupported(
            "filter subtype is outside the TS-only tuner_hal2 profile",
        )),
    }
}

pub fn build_open_filter_request(
    filter_type: &DemuxFilterType,
    buffer_size: i32,
    callback_present: bool,
) -> Result<OpenFilterRequest, HalError> {
    if buffer_size <= 0 {
        return Err(invalid("filter buffer size must be positive"));
    }
    Ok(OpenFilterRequest {
        open_type: filter_open_type(filter_type)?,
        buffer_size,
        callback_present,
    })
}

pub fn validate_ts_pid(pid: i32) -> Result<(), HalError> {
    if (0..=0x1fff).contains(&pid) {
        Ok(())
    } else {
        Err(invalid("TS PID must be 0..=0x1fff"))
    }
}

pub fn normalize_pes_stream_id(stream_id: i32) -> Result<i32, HalError> {
    if stream_id == PES_STREAM_ID_WILDCARD || (0..=0xff).contains(&stream_id) {
        Ok(stream_id)
    } else {
        Err(invalid("PES stream id must be -1 or 0..=255"))
    }
}

pub fn build_section_condition_kind(
    condition: &DemuxFilterSectionSettingsCondition,
) -> SectionConditionKind {
    match condition {
        DemuxFilterSectionSettingsCondition::SectionBits(_) => SectionConditionKind::SectionBits,
        DemuxFilterSectionSettingsCondition::TableInfo(_) => SectionConditionKind::TableInfo,
    }
}

fn normalize_table_info_version(version: i32) -> Result<Option<i32>, HalError> {
    if version == -1 {
        Ok(None)
    } else if (0..=31).contains(&version) {
        Ok(Some(version))
    } else {
        Err(invalid("section version must be -1 or 0..=31"))
    }
}

fn normalize_section_table_id(table_id: i32) -> Result<i32, HalError> {
    if (0..=255).contains(&table_id) {
        Ok(table_id)
    } else {
        Err(invalid("section table id must be 0..=255"))
    }
}

fn build_section_bits_condition(
    bits: &DemuxFilterSectionBits,
) -> Result<SectionCondition, HalError> {
    if bits.filter.len() > MAX_SECTION_FILTER_BYTES
        || bits.mask.len() > MAX_SECTION_FILTER_BYTES
        || bits.mode.len() > MAX_SECTION_FILTER_BYTES
    {
        return Err(invalid("section filter/mask/mode exceeds maximum length"));
    }
    Ok(SectionCondition {
        kind: SectionConditionKind::SectionBits,
        filter: bits.filter.clone(),
        mask: bits.mask.clone(),
        mode: bits.mode.clone(),
        table_id: None,
        version: None,
    })
}

fn build_table_info_condition(
    table: &DemuxFilterSectionSettingsConditionTableInfo,
) -> Result<SectionCondition, HalError> {
    let table_id = normalize_section_table_id(table.tableId)?;
    let version = normalize_table_info_version(table.version)?;
    Ok(SectionCondition {
        kind: SectionConditionKind::TableInfo,
        filter: vec![table_id as u8],
        mask: vec![0xff],
        mode: vec![0],
        table_id: Some(table_id),
        version,
    })
}

pub fn build_section_condition(
    condition: &DemuxFilterSectionSettingsCondition,
) -> Result<SectionCondition, HalError> {
    match condition {
        DemuxFilterSectionSettingsCondition::SectionBits(bits) => {
            build_section_bits_condition(bits)
        }
        DemuxFilterSectionSettingsCondition::TableInfo(table) => build_table_info_condition(table),
    }
}

fn supported_record_ts_index_mask() -> i32 {
    DEMUX_TS_INDEX_FIRST_PACKET
        | DEMUX_TS_INDEX_PAYLOAD_UNIT_START
        | DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED
        | DEMUX_TS_INDEX_DISCONTINUITY
        | DEMUX_TS_INDEX_RANDOM_ACCESS
        | DEMUX_TS_INDEX_PRIORITY
        | DEMUX_TS_INDEX_PCR
        | DEMUX_TS_INDEX_OPCR
        | DEMUX_TS_INDEX_SPLICING_POINT
        | DEMUX_TS_INDEX_PRIVATE_DATA
        | DEMUX_TS_INDEX_ADAPTATION_EXTENSION
}

fn record_sc_mask_variant_type(mask: &DemuxFilterScIndexMask) -> (i32, i32) {
    match mask {
        DemuxFilterScIndexMask::ScIndex(v) => (RECORD_SC_TYPE_SC, *v),
        DemuxFilterScIndexMask::ScAvc(v) => (RECORD_SC_TYPE_SC_AVC, *v),
        DemuxFilterScIndexMask::ScHevc(v) => (RECORD_SC_TYPE_SC_HEVC, *v),
        DemuxFilterScIndexMask::ScVvc(v) => (RECORD_SC_TYPE_SC_VVC, *v),
    }
}

fn supported_record_sc_index_mask(sc_index_type: i32) -> Option<i32> {
    match sc_index_type {
        RECORD_SC_TYPE_NONE => Some(0),
        RECORD_SC_TYPE_SC => Some((1 << 0) | (1 << 1) | (1 << 2) | (1 << 3)),
        RECORD_SC_TYPE_SC_AVC => Some(
            AVC_SC_I_SLICE | AVC_SC_P_SLICE | AVC_SC_B_SLICE | AVC_SC_SI_SLICE | AVC_SC_SP_SLICE,
        ),
        RECORD_SC_TYPE_SC_HEVC => Some(
            HEVC_SC_SPS
                | HEVC_SC_AUD
                | HEVC_SC_BLA_W_LP
                | HEVC_SC_BLA_W_RADL
                | HEVC_SC_BLA_N_LP
                | HEVC_SC_IDR_W_RADL
                | HEVC_SC_IDR_N_LP
                | HEVC_SC_TRAIL_CRA,
        ),
        RECORD_SC_TYPE_SC_VVC => Some(
            VVC_SC_IDR_W_RADL
                | VVC_SC_IDR_N_LP
                | VVC_SC_CRA
                | VVC_SC_GDR
                | VVC_SC_VPS
                | VVC_SC_SPS
                | VVC_SC_AUD,
        ),
        _ => None,
    }
}

pub fn validate_record_index_settings(
    ts_index_mask: i32,
    sc_index_type: i32,
    sc_index_mask: &DemuxFilterScIndexMask,
) -> Result<i32, HalError> {
    if ts_index_mask < 0 || (ts_index_mask & !supported_record_ts_index_mask()) != 0 {
        return Err(invalid("record.tsIndexMask contains unsupported bits"));
    }
    let (variant_type, mask_bits) = record_sc_mask_variant_type(sc_index_mask);
    if sc_index_type == RECORD_SC_TYPE_NONE {
        if mask_bits == 0 {
            return Ok(0);
        }
        return Err(invalid(
            "record.scIndexMask must be zero when scIndexType is NONE",
        ));
    }
    let Some(supported_mask) = supported_record_sc_index_mask(sc_index_type) else {
        return Err(invalid("record.scIndexType is unsupported"));
    };
    if variant_type != sc_index_type {
        return Err(invalid(
            "record.scIndexType does not match scIndexMask union variant",
        ));
    }
    if mask_bits < 0 || (mask_bits & !supported_mask) != 0 {
        return Err(invalid("record.scIndexMask contains unsupported bits"));
    }
    Ok(mask_bits)
}

fn build_section_config(
    open_type: FilterOpenType,
    tpid: i32,
    settings: &DemuxFilterSectionSettings,
) -> Result<FilterConfig, HalError> {
    let length_field_bits = normalize_length_field_bits(settings.bitWidthOfLengthField)
        .ok_or_else(|| invalid("section bitWidthOfLengthField is unsupported"))?;
    Ok(FilterConfig {
        open_type,
        tpid,
        kind: FilterConfigKind::TsSection {
            check_crc: settings.isCheckCrc,
            repeat: settings.isRepeat,
            raw: settings.isRaw,
            length_field_bits,
            condition: build_section_condition(&settings.condition)?,
        },
    })
}

pub fn build_filter_summary_for_open_type(
    settings: &DemuxFilterSettings,
    open_type: FilterOpenType,
) -> Result<FilterConfig, HalError> {
    let DemuxFilterSettings::Ts(ts) = settings else {
        return Err(HalError::Unsupported(
            "filter settings root is outside the TS-only tuner_hal2 profile",
        ));
    };
    validate_ts_pid(ts.tpid)?;
    let tpid = ts.tpid;
    match &ts.filterSettings {
        DemuxTsFilterSettingsFilterSettings::Noinit(_) => {
            if open_type != FilterOpenType::TsRaw {
                return Err(invalid("noinit settings require TS raw filter"));
            }
            Ok(FilterConfig {
                open_type,
                tpid,
                kind: FilterConfigKind::TsRaw,
            })
        }
        DemuxTsFilterSettingsFilterSettings::Section(section) => {
            if open_type != FilterOpenType::TsSection {
                return Err(invalid("section settings require section filter"));
            }
            build_section_config(open_type, tpid, section)
        }
        DemuxTsFilterSettingsFilterSettings::Av(av) => {
            if open_type != FilterOpenType::TsAudio && open_type != FilterOpenType::TsVideo {
                return Err(invalid("AV settings require audio/video filter"));
            }
            if av.isPassthrough {
                return Err(HalError::Unsupported("AV passthrough is not implemented"));
            }
            if av.isSecureMemory {
                return Err(HalError::Unsupported("secure AV memory is not implemented"));
            }
            Ok(FilterConfig {
                open_type,
                tpid,
                kind: FilterConfigKind::TsAv(AvSettings {
                    is_passthrough: false,
                    is_secure_memory: false,
                }),
            })
        }
        DemuxTsFilterSettingsFilterSettings::PesData(pes) => {
            if open_type != FilterOpenType::TsPes {
                return Err(invalid("PES settings require PES filter"));
            }
            Ok(FilterConfig {
                open_type,
                tpid,
                kind: FilterConfigKind::TsPes(PesSettings {
                    stream_id: normalize_pes_stream_id(pes.streamId)?,
                    raw: pes.isRaw,
                }),
            })
        }
        DemuxTsFilterSettingsFilterSettings::Record(record) => {
            if open_type != FilterOpenType::TsRecord {
                return Err(invalid("record settings require record filter"));
            }
            let mask = validate_record_index_settings(
                record.tsIndexMask,
                record.scIndexType.0,
                &record.scIndexMask,
            )?;
            Ok(FilterConfig {
                open_type,
                tpid,
                kind: FilterConfigKind::TsRecord(RecordIndexSettings {
                    ts_index_mask: record.tsIndexMask,
                    sc_index_type: record.scIndexType.0,
                    sc_index_mask: mask,
                }),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        DemuxFilterMainType::DemuxFilterMainType,
        DemuxFilterSectionSettingsCondition::DemuxFilterSectionSettingsCondition,
        DemuxFilterSubType::DemuxFilterSubType, DemuxFilterType::DemuxFilterType,
        DemuxTsFilterType::DemuxTsFilterType,
    };
    #[test]
    fn invalid_pid_is_invalid_argument() {
        assert!(validate_ts_pid(0x2000).is_err());
    }

    #[test]
    fn invalid_section_condition_is_invalid_argument() {
        let condition = DemuxFilterSectionSettingsCondition::TableInfo(
            DemuxFilterSectionSettingsConditionTableInfo {
                tableId: 300,
                version: -1,
            },
        );
        assert!(build_section_condition(&condition).is_err());
    }

    #[test]
    fn invalid_pes_stream_id_is_invalid_argument() {
        assert!(normalize_pes_stream_id(256).is_err());
    }

    #[test]
    fn record_sc_type_mask_mismatch_is_invalid_argument() {
        let mask = DemuxFilterScIndexMask::ScHevc(HEVC_SC_AUD);
        assert!(validate_record_index_settings(0, RECORD_SC_TYPE_SC_AVC, &mask).is_err());
    }

    #[test]
    fn unsupported_filter_type_is_unavailable_profile() {
        let filter_type = DemuxFilterType {
            mainType: DemuxFilterMainType::MMTP,
            subType: DemuxFilterSubType::TsFilterType(DemuxTsFilterType::SECTION),
        };
        assert!(matches!(
            filter_open_type(&filter_type),
            Err(HalError::Unsupported(_))
        ));
    }
}
