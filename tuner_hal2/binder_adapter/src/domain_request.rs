use crate::aidl_method::{AidlInputField, AidlInputSnapshot};

pub use maleicacid_tuner_hal2_domain_request::{
    AidlDomainRequest, AvStreamDomainRequest, AvStreamKind, DemuxFilterDomainRequest,
    DemuxFilterRootVariant, DemuxFilterTypeDomain, DemuxOpenDvrRequest, DemuxOpenFilterRequest,
    DomainProfileSupport, DomainRequestField, DomainValueValidation, DvrDirection,
    DvrDomainRequest, DvrRuntimeRequest, FilterDelayDomainRequest, FilterDelayKind,
    GenericDomainRequest, RuntimeExecutableRequest, ScIndexMaskRequest,
    SectionBitsConditionRequest, SectionConditionRequest, TableInfoConditionRequest,
    TsAvFilterRequest, TsFilterRuntimeRequest, TsFilterSubVariant, TsNoinitFilterRequest,
    TsPesDataFilterRequest, TsPid, TsRecordFilterRequest, TsSectionFilterRequest,
    UnsupportedDemuxFilterRequest,
};

pub fn domain_request_from_snapshot(snapshot: AidlInputSnapshot) -> AidlDomainRequest {
    let source_type = snapshot.source_type;
    let fields = snapshot
        .fields
        .iter()
        .map(classify_field)
        .collect::<Vec<_>>();
    match source_type {
        "DemuxOpenFilter" => {
            AidlDomainRequest::DemuxOpenFilter(build_open_filter_request(&snapshot, fields))
        }
        "DemuxOpenDvr" => {
            AidlDomainRequest::DemuxOpenDvr(build_open_dvr_request(&snapshot, fields))
        }
        "DemuxFilterSettings" => {
            AidlDomainRequest::DemuxFilter(build_demux_filter_request(&snapshot, fields))
        }
        "DvrSettings" => AidlDomainRequest::Dvr(build_dvr_request(&snapshot, fields)),
        "AvStreamType" => AidlDomainRequest::AvStream(build_av_stream_request(&snapshot, fields)),
        "FilterDelayHint" => {
            AidlDomainRequest::FilterDelay(build_filter_delay_request(&snapshot, fields))
        }
        _ => AidlDomainRequest::Generic(GenericDomainRequest {
            source_type,
            fields,
        }),
    }
}

fn build_open_filter_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> DemuxOpenFilterRequest {
    let filter_type = match field_value(&snapshot.fields, "filter_type.domain") {
        Some("ts") => DemuxFilterTypeDomain::Ts,
        Some("section") => DemuxFilterTypeDomain::Section,
        Some("av") => DemuxFilterTypeDomain::Av,
        Some("pes_data") => DemuxFilterTypeDomain::PesData,
        Some("record") => DemuxFilterTypeDomain::Record,
        _ => DemuxFilterTypeDomain::Unknown,
    };
    DemuxOpenFilterRequest {
        filter_type,
        buffer_size: i32_field(&snapshot.fields, "buffer_size").unwrap_or_default(),
        callback_present: bool_field(&snapshot.fields, "callback_present"),
        raw_fields: fields,
    }
}

fn build_open_dvr_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> DemuxOpenDvrRequest {
    let direction = match field_value(&snapshot.fields, "dvr_direction") {
        Some("record") => DvrDirection::Record,
        Some("playback") => DvrDirection::Playback,
        _ => DvrDirection::Unknown,
    };
    DemuxOpenDvrRequest {
        direction,
        buffer_size: i32_field(&snapshot.fields, "buffer_size").unwrap_or_default(),
        callback_present: bool_field(&snapshot.fields, "callback_present"),
        raw_fields: fields,
    }
}

fn build_demux_filter_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> DemuxFilterDomainRequest {
    let root_variant = detect_demux_filter_variant(&snapshot.fields);
    if root_variant != DemuxFilterRootVariant::Ts {
        return DemuxFilterDomainRequest::UnsupportedVariant(UnsupportedDemuxFilterRequest {
            root_variant,
            reason: "outside TS-only demux profile",
            raw_fields: fields,
        });
    }

    let tpid = match field_value(&snapshot.fields, "ts.tpid")
        .and_then(|value| TsPid::parse("ts.tpid", value).ok())
    {
        Some(pid) => pid,
        None => {
            return DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::UnsupportedTsVariant(
                UnsupportedDemuxFilterRequest {
                    root_variant,
                    reason: "missing or invalid TS PID before runtime request construction",
                    raw_fields: fields,
                },
            ))
        }
    };

    match detect_ts_filter_variant(&snapshot.fields) {
        TsFilterSubVariant::Noinit => {
            DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Noinit(TsNoinitFilterRequest {
                tpid,
                raw_fields: fields,
            }))
        }
        TsFilterSubVariant::Section => {
            DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Section(TsSectionFilterRequest {
                tpid,
                is_check_crc: bool_field(&snapshot.fields, "section.isCheckCrc"),
                is_repeat: bool_field(&snapshot.fields, "section.isRepeat"),
                is_raw: bool_field(&snapshot.fields, "section.isRaw"),
                bit_width_of_length_field: i32_field(
                    &snapshot.fields,
                    "section.bitWidthOfLengthField",
                )
                .unwrap_or_default(),
                condition: section_condition_request(&snapshot.fields),
                raw_fields: fields,
            }))
        }
        TsFilterSubVariant::Av => {
            DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Av(TsAvFilterRequest {
                tpid,
                is_passthrough: bool_field(&snapshot.fields, "av.isPassthrough"),
                is_secure_memory: bool_field(&snapshot.fields, "av.isSecureMemory"),
                raw_fields: fields,
            }))
        }
        TsFilterSubVariant::PesData => {
            DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::PesData(TsPesDataFilterRequest {
                tpid,
                stream_id: i32_field(&snapshot.fields, "pesData.streamId").unwrap_or_default(),
                is_raw: bool_field(&snapshot.fields, "pesData.isRaw"),
                raw_fields: fields,
            }))
        }
        TsFilterSubVariant::Record => {
            DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Record(TsRecordFilterRequest {
                tpid,
                ts_index_mask: i32_field(&snapshot.fields, "record.tsIndexMask")
                    .unwrap_or_default(),
                sc_index_type: i32_field(&snapshot.fields, "record.scIndexType")
                    .unwrap_or_default(),
                sc_index_mask: ScIndexMaskRequest::from_debug(
                    field_value(&snapshot.fields, "record.scIndexMask").map(ToOwned::to_owned),
                ),
                raw_fields: fields,
            }))
        }
        TsFilterSubVariant::Unknown => DemuxFilterDomainRequest::Ts(
            TsFilterRuntimeRequest::UnsupportedTsVariant(UnsupportedDemuxFilterRequest {
                root_variant,
                reason: "unknown TS filter subvariant",
                raw_fields: fields,
            }),
        ),
    }
}

fn build_dvr_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> DvrDomainRequest {
    let direction = match field_value(&snapshot.fields, "variant") {
        Some("record") => DvrDirection::Record,
        Some("playback") => DvrDirection::Playback,
        _ => DvrDirection::Unknown,
    };
    if direction == DvrDirection::Unknown {
        return DvrDomainRequest::Unsupported {
            reason: "unknown DVR settings variant",
            raw_fields: fields,
        };
    }
    let (packet_size_name, low_threshold_name, high_threshold_name, status_mask_name) =
        match direction {
            DvrDirection::Record => (
                "record.packetSize",
                "record.lowThreshold",
                "record.highThreshold",
                "record.statusMask",
            ),
            DvrDirection::Playback => (
                "playback.packetSize",
                "playback.lowThreshold",
                "playback.highThreshold",
                "playback.statusMask",
            ),
            DvrDirection::Unknown => (
                "unknown.packetSize",
                "unknown.lowThreshold",
                "unknown.highThreshold",
                "unknown.statusMask",
            ),
        };
    DvrDomainRequest::Runtime(DvrRuntimeRequest {
        direction,
        packet_size: i64_field(&snapshot.fields, packet_size_name).unwrap_or_default(),
        low_threshold: i64_field(&snapshot.fields, low_threshold_name).unwrap_or_default(),
        high_threshold: i64_field(&snapshot.fields, high_threshold_name).unwrap_or_default(),
        status_mask: i32_field(&snapshot.fields, status_mask_name).unwrap_or_default(),
        raw_fields: fields,
    })
}

fn build_av_stream_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> AvStreamDomainRequest {
    let kind = match field_value(&snapshot.fields, "variant") {
        Some("video") => AvStreamKind::Video,
        Some("audio") => AvStreamKind::Audio,
        _ => AvStreamKind::Unknown,
    };
    let stream_type_hint = match kind {
        AvStreamKind::Video => {
            i32_field(&snapshot.fields, "video.stream_type_hint").unwrap_or_default()
        }
        AvStreamKind::Audio => {
            i32_field(&snapshot.fields, "audio.stream_type_hint").unwrap_or_default()
        }
        AvStreamKind::Unknown => 0,
    };
    AvStreamDomainRequest {
        kind,
        stream_type_hint,
        raw_fields: fields,
    }
}

fn build_filter_delay_request(
    snapshot: &AidlInputSnapshot,
    fields: Vec<DomainRequestField>,
) -> FilterDelayDomainRequest {
    let kind = match field_value(&snapshot.fields, "hintType.domain") {
        Some("time_delay_ms") => FilterDelayKind::TimeDelayMs,
        Some("data_size_delay_bytes") => FilterDelayKind::DataSizeDelayBytes,
        _ => FilterDelayKind::InvalidOrUnknown,
    };
    FilterDelayDomainRequest {
        kind,
        value: i32_field(&snapshot.fields, "hintValue").unwrap_or_default(),
        raw_fields: fields,
    }
}

fn detect_demux_filter_variant(fields: &[AidlInputField]) -> DemuxFilterRootVariant {
    field_value(fields, "top_variant")
        .map(|value| match value {
            "ts" => DemuxFilterRootVariant::Ts,
            "mmtp" => DemuxFilterRootVariant::Mmtp,
            "ip" => DemuxFilterRootVariant::Ip,
            "tlv" => DemuxFilterRootVariant::Tlv,
            "alp" => DemuxFilterRootVariant::Alp,
            _ => DemuxFilterRootVariant::Unknown,
        })
        .unwrap_or(DemuxFilterRootVariant::Unknown)
}

fn detect_ts_filter_variant(fields: &[AidlInputField]) -> TsFilterSubVariant {
    field_value(fields, "ts.filterSettings.variant")
        .map(|value| match value {
            "noinit" => TsFilterSubVariant::Noinit,
            "section" => TsFilterSubVariant::Section,
            "av" => TsFilterSubVariant::Av,
            "pesData" => TsFilterSubVariant::PesData,
            "record" => TsFilterSubVariant::Record,
            _ => TsFilterSubVariant::Unknown,
        })
        .unwrap_or(TsFilterSubVariant::Unknown)
}

fn section_condition_request(fields: &[AidlInputField]) -> SectionConditionRequest {
    match field_value(fields, "section.condition.variant") {
        Some("sectionBits") => SectionConditionRequest::SectionBits(SectionBitsConditionRequest {
            filter: bytes_field(fields, "section.condition.filter.hex"),
            mask: bytes_field(fields, "section.condition.mask.hex"),
            mode: bytes_field(fields, "section.condition.mode.hex"),
        }),
        Some("tableInfo") => SectionConditionRequest::TableInfo(TableInfoConditionRequest {
            table_id: i32_field(fields, "section.condition.tableInfo.tableId").unwrap_or_default(),
            version: i32_field(fields, "section.condition.tableInfo.version").unwrap_or_default(),
        }),
        _ => SectionConditionRequest::Unknown,
    }
}

fn field_value<'a>(fields: &'a [AidlInputField], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

fn bool_field(fields: &[AidlInputField], name: &str) -> bool {
    matches!(field_value(fields, name), Some("true"))
}

fn i32_field(fields: &[AidlInputField], name: &str) -> Option<i32> {
    field_value(fields, name)?.parse().ok()
}

fn i64_field(fields: &[AidlInputField], name: &str) -> Option<i64> {
    field_value(fields, name)?.parse().ok()
}

fn bytes_field(fields: &[AidlInputField], name: &'static str) -> Vec<u8> {
    let Some(value) = field_value(fields, name) else {
        return Vec::new();
    };
    decode_hex_bytes(value).unwrap_or_default()
}

fn decode_hex_bytes(value: &str) -> Result<Vec<u8>, ()> {
    if value.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i]).ok_or(())?;
        let lo = hex_nibble(bytes[i + 1]).ok_or(())?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn classify_field(field: &AidlInputField) -> DomainRequestField {
    let validation = match field.name {
        "ts.tpid" => DomainValueValidation::U16TsPid,
        "section.condition.filter.len"
        | "section.condition.mask.len"
        | "section.condition.mode.len" => DomainValueValidation::I32NonNegative,
        "buffer_size" => DomainValueValidation::I32Positive,
        "record.packetSize" | "playback.packetSize" => DomainValueValidation::I64NonNegative,
        "record.lowThreshold"
        | "record.highThreshold"
        | "playback.lowThreshold"
        | "playback.highThreshold" => DomainValueValidation::I64NonNegative,
        "hintValue" => DomainValueValidation::I32NonNegative,
        "section.isCheckCrc" | "section.isRepeat" | "section.isRaw" | "av.isPassthrough"
        | "av.isSecureMemory" | "callback_present" | "strong_present" | "handle_present" => {
            DomainValueValidation::Bool
        }
        "top_variant"
        | "ts.filterSettings.variant"
        | "section.condition.variant"
        | "variant"
        | "hintType.domain"
        | "profile_support"
        | "domain_snapshot_kind"
        | "filter_type.domain"
        | "dvr_direction" => DomainValueValidation::EnumKnown,
        "pesData.streamId"
        | "record.tsIndexMask"
        | "record.scIndexType"
        | "record.statusMask"
        | "playback.statusMask"
        | "section.bitWidthOfLengthField"
        | "section.condition.tableInfo.tableId"
        | "section.condition.tableInfo.version"
        | "video.stream_type_hint"
        | "audio.stream_type_hint" => DomainValueValidation::I32Any,
        "aidl_schema_source"
        | "aidl_schema_hash"
        | "domain_snapshot_version"
        | "domain_mapping_status"
        | "record.scIndexMask"
        | "filter_type.raw_debug"
        | "dvr_type.raw_debug" => DomainValueValidation::DebugOnly,
        _ if field.name.ends_with(".raw_debug")
            || field.name == "debug"
            || field.name == "summary" =>
        {
            DomainValueValidation::DebugOnly
        }
        _ => DomainValueValidation::DebugOnly,
    };
    DomainRequestField::new(field.name, field.value.clone(), validation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_demux_variant_is_preserved_as_unavailable_profile() {
        let snapshot = AidlInputSnapshot::from_fields(
            "DemuxFilterSettings",
            vec![AidlInputField::new("top_variant", "ip")],
        );
        let request = domain_request_from_snapshot(snapshot);
        assert_eq!(
            request.profile_support(),
            DomainProfileSupport::UnsupportedRecordThenUnavailable
        );
        assert!(matches!(
            request,
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::UnsupportedVariant(_))
        ));
    }

    #[test]
    fn ts_pid_range_is_value_validated() {
        let snapshot = AidlInputSnapshot::from_fields(
            "DemuxFilterSettings",
            vec![
                AidlInputField::new("top_variant", "ts"),
                AidlInputField::new("ts.tpid", "8191"),
            ],
        );
        let request = domain_request_from_snapshot(snapshot);
        assert!(request.validate_supported_values().is_err());
    }

    #[test]
    fn ts_section_snapshot_becomes_strong_runtime_request() {
        let snapshot = AidlInputSnapshot::from_fields(
            "DemuxFilterSettings",
            vec![
                AidlInputField::new("top_variant", "ts"),
                AidlInputField::new("ts.tpid", "256"),
                AidlInputField::new("ts.filterSettings.variant", "section"),
                AidlInputField::new("section.isCheckCrc", "true"),
                AidlInputField::new("section.isRepeat", "false"),
                AidlInputField::new("section.isRaw", "false"),
                AidlInputField::new("section.bitWidthOfLengthField", "12"),
                AidlInputField::new("section.condition.variant", "tableInfo"),
                AidlInputField::new("section.condition.tableInfo.tableId", "0"),
                AidlInputField::new("section.condition.tableInfo.version", "1"),
            ],
        );
        let request = domain_request_from_snapshot(snapshot);
        match request {
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(
                TsFilterRuntimeRequest::Section(section),
            )) => {
                assert_eq!(section.tpid, TsPid(256));
                assert!(section.is_check_crc);
                assert!(matches!(
                    section.condition,
                    SectionConditionRequest::TableInfo(_)
                ));
            }
            _ => panic!("想定外のrequest"),
        }
    }
}
