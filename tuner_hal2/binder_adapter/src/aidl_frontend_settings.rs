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

/// A syntactically valid AIDL request can still ask for a feature that the
/// current product profile does not expose. The AIDL adapter records that fact
/// instead of deciding product support itself. Service mediation owns the
/// profile decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendProfileRequirement {
    IsdbtUnsupportedBandwidth,
    IsdbtExplicitMode,
    IsdbtExplicitInversion,
    IsdbtExplicitGuardInterval,
    IsdbtServiceAreaId,
    IsdbtPartialReceptionAuto,
    IsdbtLayerModulation,
    IsdbtLayerCoderate,
    IsdbtLayerTimeInterleave,
    IsdbtExplicitSegmentCount { layer_index: usize, count: i32 },
    IsdbsExplicitModulation,
    IsdbsExplicitCoderate,
    IsdbsExplicitRolloff,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendSettingsRequest {
    pub request: FrontendTuneRequest,
    pub profile_requirements: Vec<FrontendProfileRequirement>,
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

fn classify_auto_only(
    raw: i32,
    auto: i32,
    highest_known_bit: i32,
    requirement: FrontendProfileRequirement,
    requirements: &mut Vec<FrontendProfileRequirement>,
) -> Result<(), HalError> {
    if raw == auto {
        return Ok(());
    }
    if is_single_known_enum_value(raw, highest_known_bit) {
        requirements.push(requirement);
        return Ok(());
    }
    invalid_frontend_setting("frontend setting contains a reserved enum value")
}

fn classify_isdbt_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<Vec<FrontendProfileRequirement>, HalError> {
    let mut requirements = Vec::new();

    match s.bandwidth {
        FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => {}
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ | FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => {
            requirements.push(FrontendProfileRequirement::IsdbtUnsupportedBandwidth);
        }
        _ => {
            return invalid_frontend_setting("ISDB-T bandwidth contains a reserved enum value");
        }
    }

    classify_auto_only(
        s.mode.0,
        FrontendIsdbtMode::AUTO.0,
        FrontendIsdbtMode::MODE_3.0,
        FrontendProfileRequirement::IsdbtExplicitMode,
        &mut requirements,
    )?;

    match s.inversion {
        FrontendSpectralInversion::UNDEFINED => {}
        FrontendSpectralInversion::NORMAL | FrontendSpectralInversion::INVERTED => {
            requirements.push(FrontendProfileRequirement::IsdbtExplicitInversion);
        }
        _ => return invalid_frontend_setting("ISDB-T inversion contains a reserved enum value"),
    }

    classify_auto_only(
        s.guardInterval.0,
        FrontendIsdbtGuardInterval::AUTO.0,
        1 << 7,
        FrontendProfileRequirement::IsdbtExplicitGuardInterval,
        &mut requirements,
    )?;

    if s.serviceAreaId < 0 {
        return invalid_frontend_setting("ISDB-T serviceAreaId must be non-negative");
    }
    if s.serviceAreaId > 0 {
        requirements.push(FrontendProfileRequirement::IsdbtServiceAreaId);
    }

    match s.partialReceptionFlag {
        FrontendIsdbtPartialReceptionFlag::UNDEFINED
        | FrontendIsdbtPartialReceptionFlag::FALSE
        | FrontendIsdbtPartialReceptionFlag::TRUE => {}
        FrontendIsdbtPartialReceptionFlag::AUTO => {
            requirements.push(FrontendProfileRequirement::IsdbtPartialReceptionAuto);
        }
        _ => {
            return invalid_frontend_setting(
                "ISDB-T partialReceptionFlag contains a reserved enum value",
            )
        }
    }

    // FrontendIsdbtSettings.layerSettings is an AIDL vector. ARIB has three
    // physical layers, but AOSP does not make vector cardinality a malformed
    // AIDL-input condition. Validate each entry and preserve caller order.
    for (layer_index, layer) in s.layerSettings.iter().enumerate() {
        classify_auto_only(
            layer.modulation.0,
            FrontendIsdbtModulation::AUTO.0,
            FrontendIsdbtModulation::MOD_64QAM.0,
            FrontendProfileRequirement::IsdbtLayerModulation,
            &mut requirements,
        )?;
        classify_auto_only(
            layer.coderate.0,
            FrontendIsdbtCoderate::AUTO.0,
            FrontendIsdbtCoderate::CODERATE_8_9.0,
            FrontendProfileRequirement::IsdbtLayerCoderate,
            &mut requirements,
        )?;
        classify_auto_only(
            layer.timeInterleave.0,
            FrontendIsdbtTimeInterleaveMode::AUTO.0,
            1 << 12,
            FrontendProfileRequirement::IsdbtLayerTimeInterleave,
            &mut requirements,
        )?;
        match layer.numOfSegment {
            0 | 0xFF => {}
            1..=13 => requirements.push(FrontendProfileRequirement::IsdbtExplicitSegmentCount {
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

    Ok(requirements)
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
                // 1..=13 is carried by FrontendProfileRequirement and is
                // rejected by the current service profile before this request
                // can reach a backend. Keep the common request representable
                // without inventing a backend semantic.
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
        // AUTO is a syntactically known value. Product support is recorded
        // separately by classify_isdbt_settings().
        FrontendIsdbtPartialReceptionFlag::AUTO => {
            Ok(FrontendIsdbtPartialReceptionRequirement::Unspecified)
        }
        _ => invalid_frontend_setting("ISDB-T partialReceptionFlag contains a reserved enum value"),
    }
}

fn classify_isdbs_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
) -> Result<Vec<FrontendProfileRequirement>, HalError> {
    let mut requirements = Vec::new();
    classify_auto_only(
        s.modulation.0,
        FrontendIsdbsModulation::AUTO.0,
        FrontendIsdbsModulation::MOD_TC8PSK.0,
        FrontendProfileRequirement::IsdbsExplicitModulation,
        &mut requirements,
    )?;
    classify_auto_only(
        s.coderate.0,
        FrontendIsdbsCoderate::AUTO.0,
        FrontendIsdbsCoderate::CODERATE_7_8.0,
        FrontendProfileRequirement::IsdbsExplicitCoderate,
        &mut requirements,
    )?;
    if s.symbolRate < 0 {
        return invalid_frontend_setting("ISDB-S symbolRate must be non-negative");
    }
    match s.rolloff {
        FrontendIsdbsRolloff::UNDEFINED => {}
        FrontendIsdbsRolloff::ROLLOFF_0_35 => {
            requirements.push(FrontendProfileRequirement::IsdbsExplicitRolloff);
        }
        _ => return invalid_frontend_setting("ISDB-S rolloff contains a reserved enum value"),
    }
    Ok(requirements)
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
            let profile_requirements = classify_isdbt_settings(s)?;
            Ok(FrontendSettingsRequest {
                request: FrontendTuneRequest {
                    system: FrontendSystem::IsdbT,
                    frequency: cast_u64_field(s.frequency, "isdbt.frequency")?,
                    // endFrequency is a blind-scan field. tune/non-blind
                    // request construction does not retain it.
                    end_frequency: None,
                    stream_id: None,
                    stream_id_kind: None,
                    bandwidth_hz: map_isdbt_bandwidth(s.bandwidth),
                    symbol_rate: None,
                    isdbt_layer_settings: map_isdbt_layer_settings(s)?,
                    partial_reception: map_isdbt_partial_reception(s.partialReceptionFlag)?,
                },
                profile_requirements,
            })
        }
        FrontendSettings::Isdbs(s) => {
            let profile_requirements = classify_isdbs_settings(s)?;
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
                profile_requirements,
            })
        }
        // These AIDL variants are represented in the common domain model even
        // though the current product does not export them. Product support is
        // therefore decided later by the service/runtime, not at the AIDL
        // conversion boundary.
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
                profile_requirements: Vec::new(),
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
            profile_requirements: Vec::new(),
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
            bandwidth: FrontendIsdbtBandwidth::AUTO,
            mode: FrontendIsdbtMode::AUTO,
            guardInterval: FrontendIsdbtGuardInterval::AUTO,
            serviceAreaId: 0,
            partialReceptionFlag: FrontendIsdbtPartialReceptionFlag::UNDEFINED,
            layerSettings: vec![FrontendIsdbtLayerSettings {
                modulation: FrontendIsdbtModulation::AUTO,
                coderate: FrontendIsdbtCoderate::AUTO,
                timeInterleave: FrontendIsdbtTimeInterleaveMode::AUTO,
                numOfSegment: 0,
            }],
            ..Default::default()
        }
    }

    fn valid_isdbs_settings() -> FrontendIsdbsSettings {
        FrontendIsdbsSettings {
            modulation: FrontendIsdbsModulation::AUTO,
            coderate: FrontendIsdbsCoderate::AUTO,
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
        assert!(converted.profile_requirements.is_empty());
        assert_eq!(
            converted.request.partial_reception,
            FrontendIsdbtPartialReceptionRequirement::Unspecified
        );
    }

    #[test]
    fn isdbt_explicit_segment_is_classified_and_reserved_segment_is_invalid() {
        let mut explicit = valid_isdbt_settings();
        explicit.layerSettings[0].numOfSegment = 13;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(explicit)).unwrap();
        assert_eq!(
            converted.profile_requirements,
            vec![FrontendProfileRequirement::IsdbtExplicitSegmentCount {
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
        assert!(converted.profile_requirements.is_empty());
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
            assert!(converted.profile_requirements.is_empty());
            assert_eq!(converted.request.isdbt_layer_settings.len(), count);
        }
    }

    #[test]
    fn known_explicit_isdbt_values_are_classified_not_rejected_by_adapter() {
        let mut settings = valid_isdbt_settings();
        settings.inversion = FrontendSpectralInversion::NORMAL;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(settings)).unwrap();
        assert_eq!(
            converted.profile_requirements,
            vec![FrontendProfileRequirement::IsdbtExplicitInversion]
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
            assert!(converted.profile_requirements.is_empty());
            assert_eq!(converted.request.partial_reception, expected);
        }
    }

    #[test]
    fn isdbs_explicit_rolloff_is_classified_for_service_policy() {
        let mut settings = valid_isdbs_settings();
        settings.rolloff = FrontendIsdbsRolloff::ROLLOFF_0_35;
        let converted =
            aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
        assert_eq!(
            converted.profile_requirements,
            vec![FrontendProfileRequirement::IsdbsExplicitRolloff]
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
