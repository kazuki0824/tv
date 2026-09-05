use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendIsdbsCoderate::FrontendIsdbsCoderate, FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsRolloff::FrontendIsdbsRolloff,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth, FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval, FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtPartialReceptionFlag::FrontendIsdbtPartialReceptionFlag,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendScanType::FrontendScanType, FrontendSettings::FrontendSettings,
    FrontendSpectralInversion::FrontendSpectralInversion,
};
use maleicacid_tuner_hal2_common::{
    FrontendIsdbtLayerSetting, FrontendIsdbtPartialReceptionRequirement,
    FrontendIsdbtSegmentRequest, FrontendScanMode, FrontendStreamIdKind, FrontendSystem,
    FrontendTuneRequest, HalError, HalInvalidArgumentKind,
};

const AOSP_TUNER_INVALID_STREAM_ID: i32 = 0xFFFF;

/// AIDL上で構文的に既知だが、`FrontendTuneRequest` には直接表現されない値。
/// 呼出し元が要求した内容を保持するための観測値であり、製品対応可否の判断ではない。
/// 対応可否の方針はサービス調停が所有する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendRequestedSetting {
    IsdbtBandwidthAuto,
    IsdbtExplicitBandwidth { bandwidth_hz: u32 },
    IsdbtModeAuto,
    IsdbtExplicitMode { value: i32 },
    IsdbtExplicitInversion { value: i32 },
    IsdbtGuardIntervalAuto,
    IsdbtExplicitGuardInterval { value: i32 },
    IsdbtServiceAreaId { value: i32 },
    IsdbtPartialReceptionAuto,
    IsdbtLayerModulationAuto { layer_index: usize },
    IsdbtLayerModulation { layer_index: usize, value: i32 },
    IsdbtLayerCoderateAuto { layer_index: usize },
    IsdbtLayerCoderate { layer_index: usize, value: i32 },
    IsdbtLayerTimeInterleaveAuto { layer_index: usize },
    IsdbtLayerTimeInterleave { layer_index: usize, value: i32 },
    IsdbtExplicitSegmentCount { layer_index: usize, count: i32 },
    IsdbsModulationAuto,
    IsdbsExplicitModulation { value: i32 },
    IsdbsCoderateAuto,
    IsdbsExplicitCoderate { value: i32 },
    IsdbsExplicitRolloff { value: i32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendSettingsRequest {
    pub request: FrontendTuneRequest,
    pub requested_settings: Vec<FrontendRequestedSetting>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IsdbtKnownValue {
    Unspecified,
    Auto,
    Explicit(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IsdbsKnownValue {
    Unspecified,
    Auto,
    Explicit(i32),
}

fn cast_u64_field(value: i64, field: &'static str) -> Result<u64, HalError> {
    u64::try_from(value).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            format!("{field} must be non-negative"),
        )
    })
}

fn optional_positive_symbol_rate(value: i32, field: &'static str) -> Result<Option<u32>, HalError> {
    if value < 0 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            format!("{field} must be non-negative"),
        ));
    }
    if value == 0 {
        return Ok(None);
    }
    u32::try_from(value).map(Some).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            format!("{field} does not fit u32"),
        )
    })
}

fn map_isdbt_bandwidth(bandwidth: FrontendIsdbtBandwidth) -> Option<u32> {
    match bandwidth {
        FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => Some(6_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ => Some(7_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => Some(8_000_000),
        _ => None,
    }
}

fn is_single_known_enum_value(raw: i32, highest_known_bit: i32) -> bool {
    raw == 0 || (raw > 0 && raw <= highest_known_bit && raw.count_ones() == 1)
}

fn invalid_frontend_setting<T>(detail: impl Into<String>) -> Result<T, HalError> {
    Err(HalError::invalid_argument(
        HalInvalidArgumentKind::NumericRange,
        detail,
    ))
}

fn isdbs_known_value(
    raw: i32,
    auto: i32,
    highest_known_bit: i32,
) -> Result<IsdbsKnownValue, HalError> {
    if raw == 0 {
        return Ok(IsdbsKnownValue::Unspecified);
    }
    if raw == auto {
        return Ok(IsdbsKnownValue::Auto);
    }
    if is_single_known_enum_value(raw, highest_known_bit) {
        return Ok(IsdbsKnownValue::Explicit(raw));
    }
    invalid_frontend_setting("ISDB-S setting contains a reserved enum value")
}

fn isdbt_known_value(
    raw: i32,
    auto: i32,
    highest_known_bit: i32,
) -> Result<IsdbtKnownValue, HalError> {
    if raw == 0 {
        return Ok(IsdbtKnownValue::Unspecified);
    }
    if raw == auto {
        return Ok(IsdbtKnownValue::Auto);
    }
    if is_single_known_enum_value(raw, highest_known_bit) {
        return Ok(IsdbtKnownValue::Explicit(raw));
    }
    invalid_frontend_setting("ISDB-T setting contains a reserved enum value")
}

fn classify_isdbt_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<Vec<FrontendRequestedSetting>, HalError> {
    let mut requested = Vec::new();

    match s.bandwidth {
        FrontendIsdbtBandwidth::UNDEFINED => {}
        FrontendIsdbtBandwidth::AUTO => {
            requested.push(FrontendRequestedSetting::IsdbtBandwidthAuto);
        }
        FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitBandwidth {
                bandwidth_hz: 6_000_000,
            });
        }
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitBandwidth {
                bandwidth_hz: 7_000_000,
            });
        }
        FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitBandwidth {
                bandwidth_hz: 8_000_000,
            });
        }
        _ => {
            return invalid_frontend_setting("ISDB-T bandwidth contains a reserved enum value");
        }
    }

    match isdbt_known_value(
        s.mode.0,
        FrontendIsdbtMode::AUTO.0,
        FrontendIsdbtMode::MODE_3.0,
    )? {
        IsdbtKnownValue::Unspecified => {}
        IsdbtKnownValue::Auto => requested.push(FrontendRequestedSetting::IsdbtModeAuto),
        IsdbtKnownValue::Explicit(value) => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitMode { value });
        }
    }

    match s.inversion {
        FrontendSpectralInversion::UNDEFINED => {}
        FrontendSpectralInversion::NORMAL | FrontendSpectralInversion::INVERTED => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitInversion {
                value: s.inversion.0,
            });
        }
        _ => return invalid_frontend_setting("ISDB-T inversion contains a reserved enum value"),
    }

    match isdbt_known_value(
        s.guardInterval.0,
        FrontendIsdbtGuardInterval::AUTO.0,
        1 << 7,
    )? {
        IsdbtKnownValue::Unspecified => {}
        IsdbtKnownValue::Auto => {
            requested.push(FrontendRequestedSetting::IsdbtGuardIntervalAuto);
        }
        IsdbtKnownValue::Explicit(value) => {
            requested.push(FrontendRequestedSetting::IsdbtExplicitGuardInterval { value });
        }
    }

    if s.serviceAreaId < 0 {
        return invalid_frontend_setting("ISDB-T serviceAreaId must be non-negative");
    }
    if s.serviceAreaId > 0 {
        requested.push(FrontendRequestedSetting::IsdbtServiceAreaId {
            value: s.serviceAreaId,
        });
    }

    match s.partialReceptionFlag {
        FrontendIsdbtPartialReceptionFlag::UNDEFINED
        | FrontendIsdbtPartialReceptionFlag::FALSE
        | FrontendIsdbtPartialReceptionFlag::TRUE => {}
        FrontendIsdbtPartialReceptionFlag::AUTO => {
            requested.push(FrontendRequestedSetting::IsdbtPartialReceptionAuto);
        }
        _ => {
            return invalid_frontend_setting(
                "ISDB-T partialReceptionFlag contains a reserved enum value",
            )
        }
    }

    // `FrontendIsdbtSettings.layerSettings` はAIDL上のベクタである。ARIBの物理階層は3層だが、
    // AOSPはベクタ長そのものを不正なAIDL入力条件とはしていない。
    // 各要素を検証し、呼出し元の順序を保持する。
    for (layer_index, layer) in s.layerSettings.iter().enumerate() {
        match isdbt_known_value(
            layer.modulation.0,
            FrontendIsdbtModulation::AUTO.0,
            FrontendIsdbtModulation::MOD_64QAM.0,
        )? {
            IsdbtKnownValue::Unspecified => {}
            IsdbtKnownValue::Auto => requested.push(
                FrontendRequestedSetting::IsdbtLayerModulationAuto { layer_index },
            ),
            IsdbtKnownValue::Explicit(value) => {
                requested.push(FrontendRequestedSetting::IsdbtLayerModulation {
                    layer_index,
                    value,
                });
            }
        }
        match isdbt_known_value(
            layer.coderate.0,
            FrontendIsdbtCoderate::AUTO.0,
            FrontendIsdbtCoderate::CODERATE_8_9.0,
        )? {
            IsdbtKnownValue::Unspecified => {}
            IsdbtKnownValue::Auto => requested.push(
                FrontendRequestedSetting::IsdbtLayerCoderateAuto { layer_index },
            ),
            IsdbtKnownValue::Explicit(value) => {
                requested.push(FrontendRequestedSetting::IsdbtLayerCoderate {
                    layer_index,
                    value,
                });
            }
        }
        match isdbt_known_value(
            layer.timeInterleave.0,
            FrontendIsdbtTimeInterleaveMode::AUTO.0,
            1 << 12,
        )? {
            IsdbtKnownValue::Unspecified => {}
            IsdbtKnownValue::Auto => requested.push(
                FrontendRequestedSetting::IsdbtLayerTimeInterleaveAuto { layer_index },
            ),
            IsdbtKnownValue::Explicit(value) => {
                requested.push(FrontendRequestedSetting::IsdbtLayerTimeInterleave {
                    layer_index,
                    value,
                });
            }
        }
        match layer.numOfSegment {
            0 | 0xFF => {}
            1..=13 => requested.push(FrontendRequestedSetting::IsdbtExplicitSegmentCount {
                layer_index,
                count: layer.numOfSegment,
            }),
            _ => {
                return invalid_frontend_setting(
                    "ISDB-T numOfSegment must be 0, CTS AUTO 0xFF, or 1..=13",
                );
            }
        }
    }

    Ok(requested)
}

fn map_isdbt_layer_settings(
    settings: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<Vec<FrontendIsdbtLayerSetting>, HalError> {
    settings
        .layerSettings
        .iter()
        .map(|layer| {
            let num_of_segment = match layer.numOfSegment {
                0 => FrontendIsdbtSegmentRequest::Unspecified,
                0xFF => FrontendIsdbtSegmentRequest::Auto,
                // 明示された正確なsegment数は`FrontendRequestedSetting`へ保持し、
                // backend呼出し前にサービス調停で解釈する。
                1..=13 => FrontendIsdbtSegmentRequest::Unspecified,
                _ => {
                    return invalid_frontend_setting(
                        "validated ISDB-T layer segment request changed before conversion",
                    )
                }
            };
            Ok(FrontendIsdbtLayerSetting { num_of_segment })
        })
        .collect()
}

fn map_isdbt_partial_reception(
    value: FrontendIsdbtPartialReceptionFlag,
) -> Result<FrontendIsdbtPartialReceptionRequirement, HalError> {
    match value {
        FrontendIsdbtPartialReceptionFlag::UNDEFINED => {
            Ok(FrontendIsdbtPartialReceptionRequirement::Unspecified)
        }
        FrontendIsdbtPartialReceptionFlag::FALSE => {
            Ok(FrontendIsdbtPartialReceptionRequirement::Required(false))
        }
        FrontendIsdbtPartialReceptionFlag::TRUE => {
            Ok(FrontendIsdbtPartialReceptionRequirement::Required(true))
        }
        // AUTOは構文的に既知の値である。正確な要求は`FrontendRequestedSetting`へ保持し、
        // backend向け要求では未指定のままとする。
        FrontendIsdbtPartialReceptionFlag::AUTO => {
            Ok(FrontendIsdbtPartialReceptionRequirement::Unspecified)
        }
        _ => invalid_frontend_setting("ISDB-T partialReceptionFlag contains a reserved enum value"),
    }
}

fn classify_isdbs_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
) -> Result<Vec<FrontendRequestedSetting>, HalError> {
    let mut requested = Vec::new();
    match isdbs_known_value(
        s.modulation.0,
        FrontendIsdbsModulation::AUTO.0,
        FrontendIsdbsModulation::MOD_TC8PSK.0,
    )? {
        IsdbsKnownValue::Unspecified => {}
        IsdbsKnownValue::Auto => requested.push(FrontendRequestedSetting::IsdbsModulationAuto),
        IsdbsKnownValue::Explicit(value) => {
            requested.push(FrontendRequestedSetting::IsdbsExplicitModulation { value });
        }
    }
    match isdbs_known_value(
        s.coderate.0,
        FrontendIsdbsCoderate::AUTO.0,
        FrontendIsdbsCoderate::CODERATE_7_8.0,
    )? {
        IsdbsKnownValue::Unspecified => {}
        IsdbsKnownValue::Auto => requested.push(FrontendRequestedSetting::IsdbsCoderateAuto),
        IsdbsKnownValue::Explicit(value) => {
            requested.push(FrontendRequestedSetting::IsdbsExplicitCoderate { value });
        }
    }
    if s.symbolRate < 0 {
        return invalid_frontend_setting("ISDB-S symbolRate must be non-negative");
    }
    match s.rolloff {
        FrontendIsdbsRolloff::UNDEFINED => {}
        FrontendIsdbsRolloff::ROLLOFF_0_35 => {
            requested.push(FrontendRequestedSetting::IsdbsExplicitRolloff {
                value: s.rolloff.0,
            });
        }
        _ => return invalid_frontend_setting("ISDB-S rolloff contains a reserved enum value"),
    }
    Ok(requested)
}

fn map_isdbs_stream_selector(
    stream_id: i32,
    stream_id_type: FrontendIsdbsStreamIdType,
) -> Result<(Option<u32>, Option<FrontendStreamIdKind>), HalError> {
    match stream_id_type {
        FrontendIsdbsStreamIdType::UNDEFINED => {
            if stream_id != 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "streamId must be 0 when streamIdType is UNDEFINED",
                ));
            }
            Ok((None, None))
        }
        FrontendIsdbsStreamIdType::STREAM_ID => {
            if stream_id == AOSP_TUNER_INVALID_STREAM_ID {
                return Ok((None, None));
            }
            if stream_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "negative ISDB-S stream selector",
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "ISDB-S stream selector out of range",
                )
            })?;
            Ok((Some(value), Some(FrontendStreamIdKind::AbsoluteStreamId)))
        }
        FrontendIsdbsStreamIdType::RELATIVE_STREAM_NUMBER => {
            if stream_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "negative ISDB-S relative stream selector",
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "ISDB-S relative stream selector out of range",
                )
            })?;
            Ok((
                Some(value),
                Some(FrontendStreamIdKind::RelativeStreamNumber),
            ))
        }
        _ => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedStreamSelector,
            "unsupported ISDB-S streamIdType",
        )),
    }
}

pub fn aidl_frontend_settings_to_request(
    settings: &FrontendSettings,
) -> Result<FrontendSettingsRequest, HalError> {
    match settings {
        FrontendSettings::Isdbt(s) => {
            let requested_settings = classify_isdbt_settings(s)?;
            Ok(FrontendSettingsRequest {
                request: FrontendTuneRequest {
                    system: FrontendSystem::IsdbT,
                    frequency: cast_u64_field(s.frequency, "isdbt.frequency")?,
                    // endFrequency は blind scan 専用であり、
                    // tune/non-blind request へ保持しない。
                    end_frequency: None,
                    stream_id: None,
                    stream_id_kind: None,
                    bandwidth_hz: map_isdbt_bandwidth(s.bandwidth),
                    symbol_rate: None,
                    isdbt_layer_settings: map_isdbt_layer_settings(s)?,
                    partial_reception: map_isdbt_partial_reception(s.partialReceptionFlag)?,
                },
                requested_settings,
            })
        }
        FrontendSettings::Isdbs(s) => {
            let requested_settings = classify_isdbs_settings(s)?;
            let frequency = cast_u64_field(s.frequency, "isdbs.frequency")?;
            let symbol_rate = optional_positive_symbol_rate(s.symbolRate, "isdbs.symbolRate")?;
            let (stream_id, stream_id_kind) =
                map_isdbs_stream_selector(s.streamId, s.streamIdType)?;
            Ok(FrontendSettingsRequest {
                request: FrontendTuneRequest {
                    system: FrontendSystem::IsdbS,
                    frequency,
                    end_frequency: None,
                    stream_id,
                    stream_id_kind,
                    bandwidth_hz: None,
                    symbol_rate,
                    isdbt_layer_settings: Vec::new(),
                    partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
                },
                requested_settings,
            })
        }
        // これらのAIDL variantは現行製品では公開しないが、common domain modelには表現がある。
        // そのため製品対応可否はAIDL変換境界ではなく、後段のservice/runtimeで判定する。
        FrontendSettings::Isdbs3(s) => {
            let frequency = cast_u64_field(s.frequency, "isdbs3.frequency")?;
            let symbol_rate = optional_positive_symbol_rate(s.symbolRate, "isdbs3.symbolRate")?;
            let (stream_id, stream_id_kind) =
                map_isdbs_stream_selector(s.streamId, s.streamIdType)?;
            Ok(FrontendSettingsRequest {
                request: FrontendTuneRequest {
                    system: FrontendSystem::IsdbS3,
                    frequency,
                    end_frequency: None,
                    stream_id,
                    stream_id_kind,
                    bandwidth_hz: None,
                    symbol_rate,
                    isdbt_layer_settings: Vec::new(),
                    partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
                },
                requested_settings: Vec::new(),
            })
        }
        FrontendSettings::Dvbs(s) => Ok(FrontendSettingsRequest {
            request: FrontendTuneRequest {
                system: FrontendSystem::DvbS,
                frequency: cast_u64_field(s.frequency, "dvbs.frequency")?,
                end_frequency: None,
                stream_id: None,
                stream_id_kind: None,
                bandwidth_hz: None,
                symbol_rate: optional_positive_symbol_rate(s.symbolRate, "dvbs.symbolRate")?,
                isdbt_layer_settings: Vec::new(),
                partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
            },
            requested_settings: Vec::new(),
        }),
        _ => Err(HalError::Unsupported(
            "frontend setting variant has no tuner_hal2 domain representation",
        )),
    }
}

pub fn aidl_scan_type_to_mode(scan_type: FrontendScanType) -> Result<FrontendScanMode, HalError> {
    match scan_type {
        FrontendScanType::SCAN_AUTO => Ok(FrontendScanMode::Auto),
        FrontendScanType::SCAN_BLIND => Ok(FrontendScanMode::Blind),
        FrontendScanType::SCAN_UNDEFINED => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "scan type must be SCAN_AUTO or SCAN_BLIND",
        )),
        _ => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "frontend scan type contains a reserved enum value",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        FrontendIsdbsSettings::FrontendIsdbsSettings,
        FrontendIsdbtLayerSettings::FrontendIsdbtLayerSettings,
        FrontendIsdbtSettings::FrontendIsdbtSettings,
    };

    fn valid_isdbt_settings() -> FrontendIsdbtSettings {
        FrontendIsdbtSettings {
            inversion: FrontendSpectralInversion::UNDEFINED,
            bandwidth: FrontendIsdbtBandwidth::UNDEFINED,
            mode: FrontendIsdbtMode::UNDEFINED,
            guardInterval: FrontendIsdbtGuardInterval::UNDEFINED,
            serviceAreaId: 0,
            partialReceptionFlag: FrontendIsdbtPartialReceptionFlag::UNDEFINED,
            layerSettings: vec![FrontendIsdbtLayerSettings {
                modulation: FrontendIsdbtModulation::UNDEFINED,
                coderate: FrontendIsdbtCoderate::UNDEFINED,
                timeInterleave: FrontendIsdbtTimeInterleaveMode::UNDEFINED,
                numOfSegment: 0,
            }],
            ..Default::default()
        }
    }

    fn valid_isdbs_settings() -> FrontendIsdbsSettings {
        FrontendIsdbsSettings {
            modulation: FrontendIsdbsModulation::UNDEFINED,
            coderate: FrontendIsdbsCoderate::UNDEFINED,
            symbolRate: 0,
            rolloff: FrontendIsdbsRolloff::UNDEFINED,
            ..Default::default()
        }
    }

    #[test]
    fn isdbt_unspecified_constraints_are_accepted() {
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(valid_isdbt_settings()))
                .unwrap();
        assert!(converted.requested_settings.is_empty());
        assert_eq!(
            converted.request.partial_reception,
            FrontendIsdbtPartialReceptionRequirement::Unspecified
        );
    }

    #[test]
    fn isdbt_auto_constraints_are_distinguished_from_undefined() {
        let mut settings = valid_isdbt_settings();
        settings.bandwidth = FrontendIsdbtBandwidth::AUTO;
        settings.mode = FrontendIsdbtMode::AUTO;
        settings.guardInterval = FrontendIsdbtGuardInterval::AUTO;
        settings.layerSettings[0].modulation = FrontendIsdbtModulation::AUTO;
        settings.layerSettings[0].coderate = FrontendIsdbtCoderate::AUTO;
        settings.layerSettings[0].timeInterleave = FrontendIsdbtTimeInterleaveMode::AUTO;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
        assert_eq!(
            converted.requested_settings,
            vec![
                FrontendRequestedSetting::IsdbtBandwidthAuto,
                FrontendRequestedSetting::IsdbtModeAuto,
                FrontendRequestedSetting::IsdbtGuardIntervalAuto,
                FrontendRequestedSetting::IsdbtLayerModulationAuto { layer_index: 0 },
                FrontendRequestedSetting::IsdbtLayerCoderateAuto { layer_index: 0 },
                FrontendRequestedSetting::IsdbtLayerTimeInterleaveAuto { layer_index: 0 },
            ]
        );
        assert_eq!(converted.request.bandwidth_hz, None);
    }

    #[test]
    fn explicit_bandwidth_is_preserved_without_adapter_support_policy() {
        for (bandwidth, bandwidth_hz) in [
            (FrontendIsdbtBandwidth::BANDWIDTH_6MHZ, 6_000_000),
            (FrontendIsdbtBandwidth::BANDWIDTH_7MHZ, 7_000_000),
            (FrontendIsdbtBandwidth::BANDWIDTH_8MHZ, 8_000_000),
        ] {
            let mut settings = valid_isdbt_settings();
            settings.bandwidth = bandwidth;
            let converted =
                aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
            assert_eq!(converted.request.bandwidth_hz, Some(bandwidth_hz));
            assert_eq!(
                converted.requested_settings,
                vec![FrontendRequestedSetting::IsdbtExplicitBandwidth { bandwidth_hz }]
            );
        }
    }

    #[test]
    fn isdbt_explicit_segment_is_classified_and_reserved_segment_is_invalid() {
        let mut explicit = valid_isdbt_settings();
        explicit.layerSettings[0].numOfSegment = 13;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(explicit)).unwrap();
        assert_eq!(
            converted.requested_settings,
            vec![FrontendRequestedSetting::IsdbtExplicitSegmentCount {
                layer_index: 0,
                count: 13,
            }]
        );

        let mut reserved = valid_isdbt_settings();
        reserved.layerSettings[0].numOfSegment = 14;
        assert!(matches!(
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(reserved)),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn isdbt_cts_auto_segment_is_accepted() {
        let mut settings = valid_isdbt_settings();
        settings.layerSettings[0].numOfSegment = 0xFF;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
        assert!(converted.requested_settings.is_empty());
        assert_eq!(
            converted.request.isdbt_layer_settings[0].num_of_segment,
            FrontendIsdbtSegmentRequest::Auto
        );
    }

    #[test]
    fn isdbt_vector_cardinality_is_not_an_aidl_malformed_condition() {
        let mut template = valid_isdbt_settings();
        let layer = template.layerSettings.remove(0);
        for count in 0..=5 {
            let mut settings = valid_isdbt_settings();
            settings.layerSettings = (0..count)
                .map(|index| {
                    let mut entry = layer.clone();
                    entry.numOfSegment = if index % 2 == 0 { 0 } else { 0xff };
                    entry
                })
                .collect();
            let converted =
                aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
            assert!(converted.requested_settings.is_empty());
            assert_eq!(converted.request.isdbt_layer_settings.len(), count);
        }
    }

    #[test]
    fn known_explicit_isdbt_values_are_observed_not_rejected_by_adapter() {
        let mut settings = valid_isdbt_settings();
        settings.inversion = FrontendSpectralInversion::NORMAL;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
        assert_eq!(
            converted.requested_settings,
            vec![FrontendRequestedSetting::IsdbtExplicitInversion {
                value: FrontendSpectralInversion::NORMAL.0,
            }]
        );
    }

    #[test]
    fn isdbt_explicit_partial_reception_boolean_is_preserved() {
        for (flag, expected) in [
            (
                FrontendIsdbtPartialReceptionFlag::FALSE,
                FrontendIsdbtPartialReceptionRequirement::Required(false),
            ),
            (
                FrontendIsdbtPartialReceptionFlag::TRUE,
                FrontendIsdbtPartialReceptionRequirement::Required(true),
            ),
        ] {
            let mut settings = valid_isdbt_settings();
            settings.partialReceptionFlag = flag;
            let converted =
                aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
            assert!(converted.requested_settings.is_empty());
            assert_eq!(converted.request.partial_reception, expected);
        }
    }

    #[test]
    fn isdbs_undefined_constraints_are_unspecified() {
        let mut settings = valid_isdbs_settings();
        settings.frequency = 1_049_480_000;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
        assert!(converted.requested_settings.is_empty());
    }

    #[test]
    fn isdbs_auto_constraints_are_distinguished_from_undefined() {
        let mut settings = valid_isdbs_settings();
        settings.frequency = 1_049_480_000;
        settings.modulation = FrontendIsdbsModulation::AUTO;
        settings.coderate = FrontendIsdbsCoderate::AUTO;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
        assert_eq!(
            converted.requested_settings,
            vec![
                FrontendRequestedSetting::IsdbsModulationAuto,
                FrontendRequestedSetting::IsdbsCoderateAuto,
            ]
        );
    }

    #[test]
    fn isdbs_explicit_rolloff_is_observed_for_service_policy() {
        let mut settings = valid_isdbs_settings();
        settings.rolloff = FrontendIsdbsRolloff::ROLLOFF_0_35;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
        assert_eq!(
            converted.requested_settings,
            vec![FrontendRequestedSetting::IsdbsExplicitRolloff {
                value: FrontendIsdbsRolloff::ROLLOFF_0_35.0,
            }]
        );
    }

    #[test]
    fn isdbs_symbol_rate_preserves_zero_sentinel_and_positive_value() {
        let mut unspecified = valid_isdbs_settings();
        unspecified.frequency = 1_049_480_000;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(unspecified)).unwrap();
        assert_eq!(converted.request.symbol_rate, None);

        let mut explicit = valid_isdbs_settings();
        explicit.frequency = 1_049_480_000;
        explicit.symbolRate = 28_860_000;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(explicit)).unwrap();
        assert_eq!(converted.request.symbol_rate, Some(28_860_000));

        let mut negative = valid_isdbs_settings();
        negative.symbolRate = -1;
        assert!(matches!(
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(negative)),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn cs110_stream_selector_is_preserved_for_runtime_policy() {
        let mut settings = valid_isdbs_settings();
        settings.frequency = 1_613_000_000;
        settings.streamId = 1;
        settings.streamIdType = FrontendIsdbsStreamIdType::STREAM_ID;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
        assert_eq!(converted.request.stream_id, Some(1));
        assert_eq!(
            converted.request.stream_id_kind,
            Some(FrontendStreamIdKind::AbsoluteStreamId)
        );
    }

    #[test]
    fn isdbs_sdk_default_selector_is_unspecified_for_bs_and_cs110() {
        for frequency in [1_049_480_000, 1_613_000_000] {
            let mut settings = valid_isdbs_settings();
            settings.frequency = frequency;
            settings.streamId = AOSP_TUNER_INVALID_STREAM_ID;
            settings.streamIdType = FrontendIsdbsStreamIdType::STREAM_ID;
            let converted =
                aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
            assert_eq!(converted.request.stream_id, None);
            assert_eq!(converted.request.stream_id_kind, None);
        }
    }

    #[test]
    fn blind_scan_is_typed_for_service_policy_instead_of_rejected_by_adapter() {
        assert_eq!(
            aidl_scan_type_to_mode(FrontendScanType::SCAN_BLIND),
            Ok(FrontendScanMode::Blind)
        );
    }
}
