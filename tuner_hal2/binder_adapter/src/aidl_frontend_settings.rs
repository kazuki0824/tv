use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendIsdbsCoderate::FrontendIsdbsCoderate, FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth, FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval, FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
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

fn optional_positive_i64_to_u64_field(
    value: i64,
    field: &'static str,
) -> Result<Option<u64>, HalError> {
    if value < 0 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            format!("{field} must be non-negative"),
        ));
    }
    Ok(u64::try_from(value).ok().filter(|v| *v > 0))
}

fn map_isdbt_bandwidth(bandwidth: FrontendIsdbtBandwidth) -> Option<u32> {
    match bandwidth {
        FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => Some(6_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ => Some(7_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => Some(8_000_000),
        _ => None,
    }
}

fn validate_isdbt_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<(), HalError> {
    if !matches!(
        s.bandwidth,
        FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "ISDB-T bandwidth must be AUTO or 6MHz",
        ));
    }
    if !matches!(s.mode, FrontendIsdbtMode::AUTO | FrontendIsdbtMode::MODE_3) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "ISDB-T mode must be AUTO or MODE_3",
        ));
    }
    if !matches!(
        s.guardInterval,
        FrontendIsdbtGuardInterval::AUTO
            | FrontendIsdbtGuardInterval::INTERVAL_1_32
            | FrontendIsdbtGuardInterval::INTERVAL_1_16
            | FrontendIsdbtGuardInterval::INTERVAL_1_8
            | FrontendIsdbtGuardInterval::INTERVAL_1_4
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "unsupported ISDB-T guard interval",
        ));
    }
    for layer in &s.layerSettings {
        if !matches!(
            layer.modulation,
            FrontendIsdbtModulation::AUTO
                | FrontendIsdbtModulation::MOD_DQPSK
                | FrontendIsdbtModulation::MOD_QPSK
                | FrontendIsdbtModulation::MOD_16QAM
                | FrontendIsdbtModulation::MOD_64QAM
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer modulation",
            ));
        }
        if !matches!(
            layer.coderate,
            FrontendIsdbtCoderate::AUTO
                | FrontendIsdbtCoderate::CODERATE_1_2
                | FrontendIsdbtCoderate::CODERATE_2_3
                | FrontendIsdbtCoderate::CODERATE_3_4
                | FrontendIsdbtCoderate::CODERATE_5_6
                | FrontendIsdbtCoderate::CODERATE_7_8
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer coderate",
            ));
        }
        if !matches!(
            layer.timeInterleave,
            FrontendIsdbtTimeInterleaveMode::AUTO
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_0
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_1
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_2
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_4
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer time interleave",
            ));
        }
    }
    Ok(())
}

fn validate_isdbs_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
) -> Result<(), HalError> {
    if !matches!(
        s.modulation,
        FrontendIsdbsModulation::AUTO
            | FrontendIsdbsModulation::MOD_BPSK
            | FrontendIsdbsModulation::MOD_QPSK
            | FrontendIsdbsModulation::MOD_TC8PSK
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "unsupported ISDB-S modulation",
        ));
    }
    if !matches!(
        s.coderate,
        FrontendIsdbsCoderate::AUTO
            | FrontendIsdbsCoderate::CODERATE_1_2
            | FrontendIsdbsCoderate::CODERATE_2_3
            | FrontendIsdbsCoderate::CODERATE_3_4
            | FrontendIsdbsCoderate::CODERATE_5_6
            | FrontendIsdbsCoderate::CODERATE_7_8
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "unsupported ISDB-S coderate",
        ));
    }
    if s.symbolRate != 0 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "ISDB-S symbolRate must be 0 in this product scope",
        ));
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
                end_frequency: optional_positive_i64_to_u64_field(
                    s.endFrequency,
                    "isdbt.endFrequency",
                )?,
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
                end_frequency: optional_positive_i64_to_u64_field(
                    s.endFrequency,
                    "isdbs.endFrequency",
                )?,
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
