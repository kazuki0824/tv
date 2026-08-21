use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendIsdbsCoderate::FrontendIsdbsCoderate, FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsRolloff::FrontendIsdbsRolloff,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth, FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval, FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtPartialReceptionFlag::FrontendIsdbtPartialReceptionFlag,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendSpectralInversion::FrontendSpectralInversion,
    FrontendScanType::FrontendScanType, FrontendSettings::FrontendSettings,
};
use maleicacid_tuner_hal2_common::{
    is_japan_cs110_if_frequency_hz, FrontendScanMode, FrontendStreamIdKind, FrontendSystem,
    FrontendTuneRequest, HalError, HalInvalidArgumentKind,
};

const AOSP_TUNER_INVALID_STREAM_ID: i32 = 0xFFFF;

fn cast_u64_field(value: i64, field: &'static str) -> Result<u64, HalError> {
    u64::try_from(value).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            format!("{field} must be non-negative"),
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
    raw == 0 || (raw > 0 && raw <= highest_known_bit && raw.is_power_of_two())
}

fn unsupported_frontend_setting(
    feature: &'static str,
    detail: &'static str,
) -> Result<(), HalError> {
    Err(HalError::unsupported_detail(feature, detail))
}

fn invalid_frontend_setting(detail: &'static str) -> Result<(), HalError> {
    Err(HalError::invalid_argument(
        HalInvalidArgumentKind::NumericRange,
        detail,
    ))
}

fn validate_auto_only(
    raw: i32,
    auto: i32,
    highest_known_bit: i32,
    feature: &'static str,
    detail: &'static str,
) -> Result<(), HalError> {
    if raw == auto {
        return Ok(());
    }
    if is_single_known_enum_value(raw, highest_known_bit) {
        return unsupported_frontend_setting(feature, detail);
    }
    invalid_frontend_setting("frontend setting contains a reserved enum value")
}

fn validate_isdbt_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<(), HalError> {
    if !matches!(
        s.bandwidth,
        FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ
    ) {
        if is_single_known_enum_value(
            s.bandwidth.0,
            FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0,
        ) {
            unsupported_frontend_setting(
                "isdbt.bandwidth",
                "known ISDB-T bandwidth is not supported by this product profile",
            )?;
        }
        return invalid_frontend_setting("ISDB-T bandwidth contains a reserved enum value");
    }
    validate_auto_only(
        s.mode.0,
        FrontendIsdbtMode::AUTO.0,
        FrontendIsdbtMode::MODE_3.0,
        "isdbt.mode",
        "explicit ISDB-T mode is not supported",
    )?;
    match s.inversion {
        FrontendSpectralInversion::UNDEFINED => {}
        FrontendSpectralInversion::NORMAL | FrontendSpectralInversion::INVERTED => {
            unsupported_frontend_setting(
                "isdbt.inversion",
                "explicit ISDB-T spectral inversion is not supported",
            )?;
        }
        _ => return invalid_frontend_setting("ISDB-T inversion contains a reserved enum value"),
    }
    validate_auto_only(
        s.guardInterval.0,
        FrontendIsdbtGuardInterval::AUTO.0,
        1 << 7,
        "isdbt.guardInterval",
        "explicit ISDB-T guard interval is not supported",
    )?;
    if s.serviceAreaId < 0 {
        return invalid_frontend_setting("ISDB-T serviceAreaId must be non-negative");
    }
    if s.serviceAreaId > 0 {
        unsupported_frontend_setting(
            "isdbt.serviceAreaId",
            "explicit ISDB-T serviceAreaId is not supported",
        )?;
    }
    match s.partialReceptionFlag {
        FrontendIsdbtPartialReceptionFlag::UNDEFINED => {}
        FrontendIsdbtPartialReceptionFlag::AUTO
        | FrontendIsdbtPartialReceptionFlag::FALSE
        | FrontendIsdbtPartialReceptionFlag::TRUE => {
            unsupported_frontend_setting(
                "isdbt.partialReceptionFlag",
                "explicit ISDB-T partial reception flag is not supported",
            )?;
        }
        _ => {
            return invalid_frontend_setting(
                "ISDB-T partialReceptionFlag contains a reserved enum value",
            )
        }
    }
    for layer in &s.layerSettings {
        validate_auto_only(
            layer.modulation.0,
            FrontendIsdbtModulation::AUTO.0,
            FrontendIsdbtModulation::MOD_64QAM.0,
            "isdbt.layer.modulation",
            "explicit ISDB-T layer modulation is not supported",
        )?;
        validate_auto_only(
            layer.coderate.0,
            FrontendIsdbtCoderate::AUTO.0,
            FrontendIsdbtCoderate::CODERATE_8_9.0,
            "isdbt.layer.coderate",
            "explicit ISDB-T layer coderate is not supported",
        )?;
        validate_auto_only(
            layer.timeInterleave.0,
            FrontendIsdbtTimeInterleaveMode::AUTO.0,
            1 << 12,
            "isdbt.layer.timeInterleave",
            "explicit ISDB-T layer time interleave is not supported",
        )?;
        match layer.numOfSegment {
            0 | 0xFF => {}
            1..=13 => {
                unsupported_frontend_setting(
                    "isdbt.layer.numOfSegment",
                    "explicit ISDB-T segment count is not supported",
                )?;
            }
            _ => {
                return invalid_frontend_setting(
                    "ISDB-T numOfSegment must be 0, CTS AUTO 0xFF, or 1..=13",
                );
            }
        }
    }
    Ok(())
}

fn validate_isdbs_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
) -> Result<(), HalError> {
    validate_auto_only(
        s.modulation.0,
        FrontendIsdbsModulation::AUTO.0,
        FrontendIsdbsModulation::MOD_TC8PSK.0,
        "isdbs.modulation",
        "explicit ISDB-S modulation is not supported",
    )?;
    validate_auto_only(
        s.coderate.0,
        FrontendIsdbsCoderate::AUTO.0,
        FrontendIsdbsCoderate::CODERATE_7_8.0,
        "isdbs.coderate",
        "explicit ISDB-S coderate is not supported",
    )?;
    if s.symbolRate < 0 {
        return invalid_frontend_setting("ISDB-S symbolRate must be non-negative");
    }
    if s.symbolRate > 0 {
        unsupported_frontend_setting(
            "isdbs.symbolRate",
            "explicit ISDB-S symbolRate is not supported",
        )?;
    }
    match s.rolloff {
        FrontendIsdbsRolloff::UNDEFINED => {}
        FrontendIsdbsRolloff::ROLLOFF_0_35 => {
            unsupported_frontend_setting(
                "isdbs.rolloff",
                "explicit ISDB-S rolloff is not supported",
            )?;
        }
        _ => return invalid_frontend_setting("ISDB-S rolloff contains a reserved enum value"),
    }
    Ok(())
}

fn map_isdbs_stream_selector(
    stream_id: i32,
    stream_id_type: FrontendIsdbsStreamIdType,
    frequency_hz: u64,
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
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
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
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
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
) -> Result<FrontendTuneRequest, HalError> {
    match settings {
        FrontendSettings::Isdbt(s) => {
            validate_isdbt_fixed_settings(s)?;
            Ok(FrontendTuneRequest {
                system: FrontendSystem::IsdbT,
                frequency: cast_u64_field(s.frequency, "isdbt.frequency")?,
                // endFrequency は blind scan 専用であり、
                // tune/non-blind request へ保持しない。
                end_frequency: None,
                stream_id: None,
                stream_id_kind: None,
                bandwidth_hz: map_isdbt_bandwidth(s.bandwidth),
                symbol_rate: None,
            })
        }
        FrontendSettings::Isdbs(s) => {
            validate_isdbs_fixed_settings(s)?;
            let frequency = cast_u64_field(s.frequency, "isdbs.frequency")?;
            let (stream_id, stream_id_kind) =
                map_isdbs_stream_selector(s.streamId, s.streamIdType, frequency)?;
            Ok(FrontendTuneRequest {
                system: FrontendSystem::IsdbS,
                frequency,
                // endFrequency は blind scan 専用であり、
                // tune/non-blind request へ保持しない。
                end_frequency: None,
                stream_id,
                stream_id_kind,
                bandwidth_hz: None,
                symbol_rate: None,
            })
        }
        FrontendSettings::Isdbs3(_) => Err(HalError::Unsupported(
            "ISDB-S3 is outside the r51 product scope",
        )),
        FrontendSettings::Dvbs(_) => Err(HalError::Unsupported(
            "DVB-S is outside the r51 product scope",
        )),
        _ => Err(HalError::Unsupported(
            "frontend setting is outside the r51 product scope",
        )),
    }
}

pub fn aidl_scan_type_to_mode(scan_type: FrontendScanType) -> Result<FrontendScanMode, HalError> {
    match scan_type {
        FrontendScanType::SCAN_AUTO => Ok(FrontendScanMode::Auto),
        FrontendScanType::SCAN_BLIND => Err(HalError::Unsupported(
            "blind scan is outside the r51 product scope; TIS must submit explicit candidates",
        )),
        FrontendScanType::SCAN_UNDEFINED => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "scan type must be SCAN_AUTO or SCAN_BLIND",
        )),
        _ => Err(HalError::Unsupported(
            "frontend scan type is outside the r51 product scope",
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
    fn isdbt_auto_segment_and_unspecified_constraints_are_accepted() {
        assert_eq!(validate_isdbt_fixed_settings(&valid_isdbt_settings()), Ok(()));
    }

    #[test]
    fn isdbt_explicit_segment_is_unavailable_and_reserved_segment_is_invalid() {
        let mut explicit = valid_isdbt_settings();
        explicit.layerSettings[0].numOfSegment = 13;
        assert!(matches!(
            validate_isdbt_fixed_settings(&explicit),
            Err(HalError::UnsupportedDetail { .. })
        ));

        let mut reserved = valid_isdbt_settings();
        reserved.layerSettings[0].numOfSegment = 14;
        assert!(matches!(
            validate_isdbt_fixed_settings(&reserved),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn isdbt_cts_auto_segment_is_accepted() {
        let mut settings = valid_isdbt_settings();
        settings.layerSettings[0].numOfSegment = 0xFF;
        assert_eq!(validate_isdbt_fixed_settings(&settings), Ok(()));
    }

    #[test]
    fn end_frequency_is_not_retained_in_non_blind_request() {
        let mut isdbt = valid_isdbt_settings();
        isdbt.frequency = 473_142_857;
        isdbt.endFrequency = -1;
        let request = aidl_frontend_settings_to_request(&FrontendSettings::Isdbt(isdbt)).unwrap();
        assert_eq!(request.end_frequency, None);

        let mut isdbs = valid_isdbs_settings();
        isdbs.frequency = 1_049_480_000;
        isdbs.endFrequency = 2_053_000_000;
        isdbs.streamId = AOSP_TUNER_INVALID_STREAM_ID;
        isdbs.streamIdType = FrontendIsdbsStreamIdType::STREAM_ID;
        let request = aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(isdbs)).unwrap();
        assert_eq!(request.end_frequency, None);
    }

    #[test]
    fn isdbt_explicit_inversion_and_partial_reception_are_unavailable() {
        let mut inversion = valid_isdbt_settings();
        inversion.inversion = FrontendSpectralInversion::NORMAL;
        assert!(matches!(
            validate_isdbt_fixed_settings(&inversion),
            Err(HalError::UnsupportedDetail { .. })
        ));

        let mut partial = valid_isdbt_settings();
        partial.partialReceptionFlag = FrontendIsdbtPartialReceptionFlag::AUTO;
        assert!(matches!(
            validate_isdbt_fixed_settings(&partial),
            Err(HalError::UnsupportedDetail { .. })
        ));
    }

    #[test]
    fn isdbs_explicit_rolloff_is_unavailable() {
        let mut settings = valid_isdbs_settings();
        settings.rolloff = FrontendIsdbsRolloff::ROLLOFF_0_35;
        assert!(matches!(
            validate_isdbs_fixed_settings(&settings),
            Err(HalError::UnsupportedDetail { .. })
        ));
    }

    #[test]
    fn isdbs_sdk_default_selector_is_unspecified_for_bs_and_cs110() {
        for frequency in [1_049_480_000, 1_613_000_000] {
            let mut settings = valid_isdbs_settings();
            settings.frequency = frequency;
            settings.streamId = AOSP_TUNER_INVALID_STREAM_ID;
            settings.streamIdType = FrontendIsdbsStreamIdType::STREAM_ID;
            let request =
                aidl_frontend_settings_to_request(&FrontendSettings::Isdbs(settings)).unwrap();
            assert_eq!(request.stream_id, None);
            assert_eq!(request.stream_id_kind, None);
        }
    }
}
