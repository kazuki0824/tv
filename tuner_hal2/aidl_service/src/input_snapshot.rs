use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AvStreamType::AvStreamType,
    DemuxFilterMainType::DemuxFilterMainType,
    DemuxFilterSectionSettingsCondition::DemuxFilterSectionSettingsCondition,
    DemuxFilterSettings::DemuxFilterSettings,
    DemuxFilterSubType::DemuxFilterSubType,
    DemuxFilterType::DemuxFilterType,
    DemuxTsFilterType::DemuxTsFilterType,
    DemuxTsFilterSettingsFilterSettings::DemuxTsFilterSettingsFilterSettings,
    DvrSettings::DvrSettings,
    DvrType::DvrType,
    FilterDelayHint::FilterDelayHint,
    FilterDelayHintType::FilterDelayHintType,
};
use maleicacid_tuner_hal2_binder_adapter::{AidlInputField, AidlInputSnapshot};

use crate::aidl_v2_conversion_contract::{
    contract_fields, schema_identity_fields, unsupported_variant_contract_fields,
    AV_STREAM_AND_DELAY_FIELDS, DEMUX_FILTER_SETTINGS_UNION, DEMUX_TS_AV_PES_RECORD_FIELDS,
    DEMUX_TS_FILTER_SETTINGS_FIELDS, DEMUX_TS_SECTION_FIELDS, DVR_SETTINGS_FIELDS,
};

fn schema_field() -> AidlInputField { schema_identity_fields()[0].clone() }
fn schema_hash_field() -> AidlInputField { schema_identity_fields()[1].clone() }
fn snapshot_version_field() -> AidlInputField { schema_identity_fields()[2].clone() }

fn debug_field<T: std::fmt::Debug>(name: &'static str, value: &T) -> AidlInputField {
    AidlInputField::new(name, format!("{:?}", value))
}

fn presence_field(name: &'static str, present: bool) -> AidlInputField {
    AidlInputField::new(name, if present { "true" } else { "false" })
}

fn bool_field(name: &'static str, value: bool) -> AidlInputField {
    AidlInputField::new(name, if value { "true" } else { "false" })
}

fn i32_field(name: &'static str, value: i32) -> AidlInputField { AidlInputField::new(name, value.to_string()) }

fn demux_filter_type_domain_label(filter_type: &DemuxFilterType) -> &'static str {
    if filter_type.mainType != DemuxFilterMainType::TS {
        return "unknown";
    }

    match &filter_type.subType {
        DemuxFilterSubType::TsFilterType(ts_type) => match *ts_type {
            DemuxTsFilterType::TS => "ts",
            DemuxTsFilterType::SECTION => "section",
            DemuxTsFilterType::PES => "pes_data",
            DemuxTsFilterType::RECORD => "record",
            DemuxTsFilterType::AUDIO | DemuxTsFilterType::VIDEO => "av",
            _ => "unknown",
        },
        _ => "unknown",
    }
}

fn bytes_hex_field(name: &'static str, value: &[u8]) -> AidlInputField {
    let mut hex = String::with_capacity(value.len() * 2);
    for byte in value {
        use std::fmt::Write as _;
        if write!(&mut hex, "{:02x}", byte).is_err() {
            return AidlInputField::new(name, "");
        }
    }
    AidlInputField::new(name, hex)
}
fn i64_field(name: &'static str, value: i64) -> AidlInputField { AidlInputField::new(name, value.to_string()) }

fn base_fields(kind: &'static str) -> Vec<AidlInputField> {
    vec![
        schema_field(),
        schema_hash_field(),
        snapshot_version_field(),
        AidlInputField::new("domain_snapshot_kind", kind),
    ]
}

fn unsupported_profile_fields(top_variant: &'static str) -> Vec<AidlInputField> {
    unsupported_variant_contract_fields(top_variant)
}

pub fn snapshot_demux_open_filter(
    filter_type: &DemuxFilterType,
    buffer_size: i32,
    callback_present: bool,
) -> AidlInputSnapshot {
    let mut fields = base_fields("DemuxOpenFilter");
    fields.extend([
        debug_field("filter_type.raw_debug", filter_type),
        AidlInputField::new("filter_type.domain", demux_filter_type_domain_label(filter_type)),
        i32_field("buffer_size", buffer_size),
        presence_field("callback_present", callback_present),
        AidlInputField::new("domain_mapping_status", "filter_type_structured_domain_label"),
    ]);
    AidlInputSnapshot::from_fields("DemuxOpenFilter", fields)
}

pub fn snapshot_demux_open_dvr(
    dvr_type: DvrType,
    buffer_size: i32,
    callback_present: bool,
) -> AidlInputSnapshot {
    let mut fields = base_fields("DemuxOpenDvr");
    fields.extend([
        debug_field("dvr_type.raw_debug", &dvr_type),
        AidlInputField::new("dvr_direction", match dvr_type {
            DvrType::RECORD => "record",
            DvrType::PLAYBACK => "playback",
            _ => "unknown_or_unsupported",
        }),
        i32_field("buffer_size", buffer_size),
        presence_field("callback_present", callback_present),
    ]);
    AidlInputSnapshot::from_fields("DemuxOpenDvr", fields)
}

pub fn snapshot_filter_settings(settings: &DemuxFilterSettings) -> AidlInputSnapshot {
    let mut fields = base_fields("DemuxFilterSettings");
    fields.extend(contract_fields("DemuxFilterSettings", DEMUX_FILTER_SETTINGS_UNION));
    match settings {
        DemuxFilterSettings::Ts(ts) => {
            fields.extend([
                AidlInputField::new("top_variant", "ts"),
                AidlInputField::new("profile_support", "supported_profile_root"),
                i32_field("ts.tpid", ts.tpid),
            ]);
            fields.extend(contract_fields("DemuxTsFilterSettings", DEMUX_TS_FILTER_SETTINGS_FIELDS));
            match &ts.filterSettings {
                DemuxTsFilterSettingsFilterSettings::Noinit(_) => {
                    fields.extend([
                        AidlInputField::new("ts.filterSettings.variant", "noinit"),
                        AidlInputField::new("ts.filterSettings.open_type_requirement", "DemuxTsFilterType::TS"),
                    ]);
                }
                DemuxTsFilterSettingsFilterSettings::Section(section) => {
                    fields.extend([
                        AidlInputField::new("ts.filterSettings.variant", "section"),
                        bool_field("section.isCheckCrc", section.isCheckCrc),
                        bool_field("section.isRepeat", section.isRepeat),
                        bool_field("section.isRaw", section.isRaw),
                        i32_field("section.bitWidthOfLengthField", section.bitWidthOfLengthField),
                    ]);
                    fields.extend(contract_fields("DemuxFilterSectionSettings", DEMUX_TS_SECTION_FIELDS));
                    match &section.condition {
                        DemuxFilterSectionSettingsCondition::SectionBits(bits) => {
                            fields.extend([
                                AidlInputField::new("section.condition.variant", "sectionBits"),
                                AidlInputField::new("section.condition.filter.len", bits.filter.len().to_string()),
                                AidlInputField::new("section.condition.mask.len", bits.mask.len().to_string()),
                                AidlInputField::new("section.condition.mode.len", bits.mode.len().to_string()),
                                bytes_hex_field("section.condition.filter.hex", &bits.filter),
                                bytes_hex_field("section.condition.mask.hex", &bits.mask),
                                bytes_hex_field("section.condition.mode.hex", &bits.mode),
                            ]);
                        }
                        DemuxFilterSectionSettingsCondition::TableInfo(table) => {
                            fields.extend([
                                AidlInputField::new("section.condition.variant", "tableInfo"),
                                i32_field("section.condition.tableInfo.tableId", table.tableId),
                                i32_field("section.condition.tableInfo.version", table.version),
                            ]);
                        }
                    }
                }
                DemuxTsFilterSettingsFilterSettings::Av(av) => {
                    fields.extend([
                        AidlInputField::new("ts.filterSettings.variant", "av"),
                        bool_field("av.isPassthrough", av.isPassthrough),
                        bool_field("av.isSecureMemory", av.isSecureMemory),
                    ]);
                    fields.extend(contract_fields("DemuxFilterAvSettings", DEMUX_TS_AV_PES_RECORD_FIELDS));
                }
                DemuxTsFilterSettingsFilterSettings::PesData(pes) => {
                    fields.extend([
                        AidlInputField::new("ts.filterSettings.variant", "pesData"),
                        i32_field("pesData.streamId", pes.streamId),
                        bool_field("pesData.isRaw", pes.isRaw),
                    ]);
                    fields.extend(contract_fields("DemuxFilterPesDataSettings", DEMUX_TS_AV_PES_RECORD_FIELDS));
                }
                DemuxTsFilterSettingsFilterSettings::Record(record) => {
                    fields.extend([
                        AidlInputField::new("ts.filterSettings.variant", "record"),
                        i32_field("record.tsIndexMask", record.tsIndexMask),
                        i32_field("record.scIndexType", record.scIndexType.0),
                        debug_field("record.scIndexMask", &record.scIndexMask),
                    ]);
                    fields.extend(contract_fields("DemuxFilterRecordSettings", DEMUX_TS_AV_PES_RECORD_FIELDS));
                }
            }
        }
        DemuxFilterSettings::Mmtp(value) => {
            fields.extend(unsupported_profile_fields("mmtp"));
            fields.push(debug_field("mmtp.raw_debug", value));
        }
        DemuxFilterSettings::Ip(value) => {
            fields.extend(unsupported_profile_fields("ip"));
            fields.push(debug_field("ip.raw_debug", value));
        }
        DemuxFilterSettings::Tlv(value) => {
            fields.extend(unsupported_profile_fields("tlv"));
            fields.push(debug_field("tlv.raw_debug", value));
        }
        DemuxFilterSettings::Alp(value) => {
            fields.extend(unsupported_profile_fields("alp"));
            fields.push(debug_field("alp.raw_debug", value));
        }
    }
    AidlInputSnapshot::from_fields("DemuxFilterSettings", fields)
}

pub fn snapshot_av_stream_type(stream_type: &AvStreamType) -> AidlInputSnapshot {
    let mut fields = base_fields("AvStreamType");
    fields.extend(contract_fields("AvStreamType", AV_STREAM_AND_DELAY_FIELDS));
    match stream_type {
        AvStreamType::Video(value) => fields.extend([
            AidlInputField::new("variant", "video"),
            i32_field("video.stream_type_hint", value.0),
        ]),
        AvStreamType::Audio(value) => fields.extend([
            AidlInputField::new("variant", "audio"),
            i32_field("audio.stream_type_hint", value.0),
        ]),
    }
    AidlInputSnapshot::from_fields("AvStreamType", fields)
}

pub fn snapshot_filter_delay_hint(hint: &FilterDelayHint) -> AidlInputSnapshot {
    let mut fields = base_fields("FilterDelayHint");
    fields.extend(contract_fields("FilterDelayHint", AV_STREAM_AND_DELAY_FIELDS));
    fields.extend([
        debug_field("hintType.raw", &hint.hintType),
        i32_field("hintValue", hint.hintValue),
        AidlInputField::new("hintType.domain", match hint.hintType {
            FilterDelayHintType::TIME_DELAY_IN_MS => "time_delay_ms",
            FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES => "data_size_delay_bytes",
            _ => "invalid_or_unknown",
        }),
    ]);
    AidlInputSnapshot::from_fields("FilterDelayHint", fields)
}

pub fn snapshot_dvr_settings(settings: &DvrSettings) -> AidlInputSnapshot {
    let mut fields = base_fields("DvrSettings");
    fields.extend(contract_fields("DvrSettings", DVR_SETTINGS_FIELDS));
    match settings {
        DvrSettings::Record(record) => {
            fields.extend([
                AidlInputField::new("variant", "record"),
                debug_field("record.dataFormat", &record.dataFormat),
                i64_field("record.packetSize", record.packetSize),
                i64_field("record.lowThreshold", record.lowThreshold),
                i64_field("record.highThreshold", record.highThreshold),
                i32_field("record.statusMask", record.statusMask),
            ]);
        }
        DvrSettings::Playback(playback) => {
            fields.extend([
                AidlInputField::new("variant", "playback"),
                debug_field("playback.dataFormat", &playback.dataFormat),
                i64_field("playback.packetSize", playback.packetSize),
                i64_field("playback.lowThreshold", playback.lowThreshold),
                i64_field("playback.highThreshold", playback.highThreshold),
                i32_field("playback.statusMask", playback.statusMask),
            ]);
        }
    }
    AidlInputSnapshot::from_fields("DvrSettings", fields)
}

pub fn snapshot_strong_handle(source_type: &'static str) -> AidlInputSnapshot {
    let mut fields = base_fields(source_type);
    fields.push(presence_field("strong_present", true));
    AidlInputSnapshot::from_fields(source_type, fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_handle_snapshot_has_named_presence_field() {
        let snapshot = snapshot_strong_handle("IFilter");
        assert_eq!(snapshot.source_type, "IFilter");
        assert!(snapshot.fields.iter().any(|field| field.name == "strong_present" && field.value == "true"));
        assert!(snapshot.fields.iter().any(|field| field.name == "aidl_schema_source"));
    }

    #[test]
    fn snapshot_base_fields_include_version_and_source() {
        let fields = base_fields("unit-test");
        assert!(fields.iter().any(|field| field.name == "aidl_schema_source" && field.value.contains("android.hardware.tv.tuner-V2")));
        assert!(fields.iter().any(|field| field.name == "domain_snapshot_version" && field.value == "r50ee27"));
        assert!(fields.iter().any(|field| field.name == "aidl_schema_hash"));
    }
}
