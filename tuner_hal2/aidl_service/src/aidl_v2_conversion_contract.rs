use maleicacid_tuner_hal2_binder_adapter::AidlInputField;

pub const AIDL_V2_SCHEMA_SOURCE: &str =
    "android.hardware.tv.tuner-V2 frozen AIDL; generated from hardware/interfaces/tv/tuner/aidl/aidl_api/android.hardware.tv.tuner/2";
pub const AIDL_V2_SCHEMA_HASH: &str = "f8d74c149f04e76b6d622db2bd8e465dae24b08c";
pub const DOMAIN_SNAPSHOT_VERSION: &str = "r50ee27";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AidlFieldContract {
    pub owner: &'static str,
    pub field: &'static str,
    pub product_policy: &'static str,
    pub domain_storage: &'static str,
}

pub const DEMUX_FILTER_SETTINGS_UNION: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "DemuxFilterSettings", field: "ts", product_policy: "supported_root", domain_storage: "DemuxFilterDomain::Ts" },
    AidlFieldContract { owner: "DemuxFilterSettings", field: "mmtp", product_policy: "unsupported_variant_record_then_unavailable", domain_storage: "UnsupportedDemuxFilterVariant::Mmtp" },
    AidlFieldContract { owner: "DemuxFilterSettings", field: "ip", product_policy: "unsupported_variant_record_then_unavailable", domain_storage: "UnsupportedDemuxFilterVariant::Ip" },
    AidlFieldContract { owner: "DemuxFilterSettings", field: "tlv", product_policy: "unsupported_variant_record_then_unavailable", domain_storage: "UnsupportedDemuxFilterVariant::Tlv" },
    AidlFieldContract { owner: "DemuxFilterSettings", field: "alp", product_policy: "unsupported_variant_record_then_unavailable", domain_storage: "UnsupportedDemuxFilterVariant::Alp" },
];

pub const DEMUX_TS_FILTER_SETTINGS_FIELDS: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "tpid", product_policy: "ts_pid_value_validation_required", domain_storage: "TsFilterDomain::tpid" },
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "filterSettings.noinit", product_policy: "supported_ts_subvariant", domain_storage: "TsFilterDomain::NoInit" },
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "filterSettings.section", product_policy: "supported_ts_subvariant", domain_storage: "TsFilterDomain::Section" },
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "filterSettings.av", product_policy: "supported_ts_subvariant_non_passthrough_only", domain_storage: "TsFilterDomain::Av" },
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "filterSettings.pesData", product_policy: "supported_ts_subvariant", domain_storage: "TsFilterDomain::PesData" },
    AidlFieldContract { owner: "DemuxTsFilterSettings", field: "filterSettings.record", product_policy: "supported_ts_subvariant", domain_storage: "TsFilterDomain::Record" },
];

pub const DEMUX_TS_SECTION_FIELDS: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "DemuxFilterSectionSettings", field: "isCheckCrc", product_policy: "supported", domain_storage: "SectionDomain::is_check_crc" },
    AidlFieldContract { owner: "DemuxFilterSectionSettings", field: "isRepeat", product_policy: "supported", domain_storage: "SectionDomain::is_repeat" },
    AidlFieldContract { owner: "DemuxFilterSectionSettings", field: "isRaw", product_policy: "supported", domain_storage: "SectionDomain::is_raw" },
    AidlFieldContract { owner: "DemuxFilterSectionSettings", field: "bitWidthOfLengthField", product_policy: "validate_0_32", domain_storage: "SectionDomain::bit_width_of_length_field" },
    AidlFieldContract { owner: "DemuxFilterSectionSettingsCondition", field: "sectionBits.filter", product_policy: "supported_bytes", domain_storage: "SectionConditionDomain::filter" },
    AidlFieldContract { owner: "DemuxFilterSectionSettingsCondition", field: "sectionBits.mask", product_policy: "supported_bytes", domain_storage: "SectionConditionDomain::mask" },
    AidlFieldContract { owner: "DemuxFilterSectionSettingsCondition", field: "sectionBits.mode", product_policy: "supported_bytes", domain_storage: "SectionConditionDomain::mode" },
    AidlFieldContract { owner: "DemuxFilterSectionSettingsCondition", field: "tableInfo.tableId", product_policy: "validate_table_id", domain_storage: "SectionConditionDomain::table_id" },
    AidlFieldContract { owner: "DemuxFilterSectionSettingsCondition", field: "tableInfo.version", product_policy: "validate_version", domain_storage: "SectionConditionDomain::version" },
];

pub const DEMUX_TS_AV_PES_RECORD_FIELDS: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "DemuxFilterAvSettings", field: "isPassthrough", product_policy: "false_supported_true_unavailable", domain_storage: "AvFilterDomain::is_passthrough" },
    AidlFieldContract { owner: "DemuxFilterAvSettings", field: "isSecureMemory", product_policy: "record_for_profile_check", domain_storage: "AvFilterDomain::is_secure_memory" },
    AidlFieldContract { owner: "DemuxFilterPesDataSettings", field: "streamId", product_policy: "validate_stream_id", domain_storage: "PesDataDomain::stream_id" },
    AidlFieldContract { owner: "DemuxFilterPesDataSettings", field: "isRaw", product_policy: "supported", domain_storage: "PesDataDomain::is_raw" },
    AidlFieldContract { owner: "DemuxFilterRecordSettings", field: "tsIndexMask", product_policy: "supported_mask", domain_storage: "RecordFilterDomain::ts_index_mask" },
    AidlFieldContract { owner: "DemuxFilterRecordSettings", field: "scIndexType", product_policy: "supported_enum", domain_storage: "RecordFilterDomain::sc_index_type" },
    AidlFieldContract { owner: "DemuxFilterRecordSettings", field: "scIndexMask", product_policy: "supported_union_or_mask", domain_storage: "RecordFilterDomain::sc_index_mask" },
];

pub const DVR_SETTINGS_FIELDS: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "DvrSettings", field: "record.dataFormat", product_policy: "supported_data_format", domain_storage: "DvrRecordDomain::data_format" },
    AidlFieldContract { owner: "DvrSettings", field: "record.packetSize", product_policy: "validate_positive", domain_storage: "DvrRecordDomain::packet_size" },
    AidlFieldContract { owner: "DvrSettings", field: "record.lowThreshold", product_policy: "validate_non_negative", domain_storage: "DvrRecordDomain::low_threshold" },
    AidlFieldContract { owner: "DvrSettings", field: "record.highThreshold", product_policy: "validate_non_negative", domain_storage: "DvrRecordDomain::high_threshold" },
    AidlFieldContract { owner: "DvrSettings", field: "record.statusMask", product_policy: "supported_mask", domain_storage: "DvrRecordDomain::status_mask" },
    AidlFieldContract { owner: "DvrSettings", field: "playback.dataFormat", product_policy: "supported_data_format", domain_storage: "DvrPlaybackDomain::data_format" },
    AidlFieldContract { owner: "DvrSettings", field: "playback.packetSize", product_policy: "validate_positive", domain_storage: "DvrPlaybackDomain::packet_size" },
    AidlFieldContract { owner: "DvrSettings", field: "playback.lowThreshold", product_policy: "validate_non_negative", domain_storage: "DvrPlaybackDomain::low_threshold" },
    AidlFieldContract { owner: "DvrSettings", field: "playback.highThreshold", product_policy: "validate_non_negative", domain_storage: "DvrPlaybackDomain::high_threshold" },
    AidlFieldContract { owner: "DvrSettings", field: "playback.statusMask", product_policy: "supported_mask", domain_storage: "DvrPlaybackDomain::status_mask" },
];

pub const AV_STREAM_AND_DELAY_FIELDS: &[AidlFieldContract] = &[
    AidlFieldContract { owner: "AvStreamType", field: "video", product_policy: "supported_enum", domain_storage: "AvStreamDomain::Video" },
    AidlFieldContract { owner: "AvStreamType", field: "audio", product_policy: "supported_enum", domain_storage: "AvStreamDomain::Audio" },
    AidlFieldContract { owner: "FilterDelayHint", field: "hintType", product_policy: "time_or_size_supported_other_invalid", domain_storage: "FilterDelayDomain::hint_type" },
    AidlFieldContract { owner: "FilterDelayHint", field: "hintValue", product_policy: "validate_non_negative", domain_storage: "FilterDelayDomain::hint_value" },
];

pub fn schema_identity_fields() -> [AidlInputField; 3] {
    [
        AidlInputField::new("aidl_schema_source", AIDL_V2_SCHEMA_SOURCE),
        AidlInputField::new("aidl_schema_hash", AIDL_V2_SCHEMA_HASH),
        AidlInputField::new("domain_snapshot_version", DOMAIN_SNAPSHOT_VERSION),
    ]
}

pub fn contract_fields(prefix: &'static str, contracts: &[AidlFieldContract]) -> Vec<AidlInputField> {
    let mut fields = Vec::with_capacity(contracts.len() * 3);
    for contract in contracts {
        fields.push(AidlInputField::new("field_contract.owner", format!("{prefix}.{}", contract.owner)));
        fields.push(AidlInputField::new("field_contract.field", contract.field));
        fields.push(AidlInputField::new("field_contract.storage", contract.domain_storage));
    }
    fields
}

pub fn unsupported_variant_contract_fields(top_variant: &'static str) -> Vec<AidlInputField> {
    let mut fields = vec![
        AidlInputField::new("top_variant", top_variant),
        AidlInputField::new("profile_support", "unsupported"),
        AidlInputField::new("unsupported_policy", "record_variant_then_return_unavailable"),
        AidlInputField::new("unsupported_precedence", "profile_unsupported_before_field_value_validation"),
    ];
    fields.extend(contract_fields("DemuxFilterSettings", DEMUX_FILTER_SETTINGS_UNION));
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demux_filter_union_contract_covers_android14_aidl_v2_tags() {
        let tags: Vec<&str> = DEMUX_FILTER_SETTINGS_UNION.iter().map(|entry| entry.field).collect();
        assert_eq!(tags, vec!["ts", "mmtp", "ip", "tlv", "alp"]);
    }

    #[test]
    fn ts_contract_covers_all_supported_product_subvariants() {
        for field in ["filterSettings.noinit", "filterSettings.section", "filterSettings.av", "filterSettings.pesData", "filterSettings.record"] {
            assert!(DEMUX_TS_FILTER_SETTINGS_FIELDS.iter().any(|entry| entry.field == field), "missing {field}");
        }
    }

    #[test]
    fn dvr_contract_covers_record_and_playback() {
        assert!(DVR_SETTINGS_FIELDS.iter().any(|entry| entry.field == "record.packetSize"));
        assert!(DVR_SETTINGS_FIELDS.iter().any(|entry| entry.field == "playback.packetSize"));
    }
}
