//! DVB / earth_pt1 explicit tune mapping断片。
//!
//! requestからDTV propertyへ変換する再利用logicだけを置く。旧frontend backend lifecycleやioctl実行層はコピーしない。

use maleicacid_tuner_hal2_common::{
    is_japan_bs_if_frequency_hz, is_japan_cs110_if_frequency_hz,
    is_japan_isdbt_frequency_contract_hz, FrontendStreamIdKind, FrontendSystem,
    FrontendTuneRequest, HalError, HalInvalidArgumentKind,
};

use crate::dvb::abi::{
    DtvProperty, DTV_BANDWIDTH_HZ, DTV_DELIVERY_SYSTEM, DTV_FREQUENCY, DTV_STREAM_ID, DTV_TUNE,
    SYS_DVBS2, SYS_ISDBS, SYS_ISDBT,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvbTuneRequest {
    pub frequency_hz: Option<u32>,
    pub stream_id: Option<u16>,
    pub stream_id_kind: Option<FrontendStreamIdKind>,
    pub bandwidth_hz: Option<u32>,
    pub symbol_rate: Option<u32>,
    pub system: Option<FrontendSystem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvbTunePropertyPairs {
    pub pairs: Vec<(u32, u32)>,
}

impl DvbTunePropertyPairs {
    pub fn to_dtv_properties(&self) -> Vec<DtvProperty> {
        self.pairs
            .iter()
            .copied()
            .map(|(cmd, value)| DtvProperty::with_data(cmd, value))
            .collect()
    }
}

pub fn delivery_system(system: Option<FrontendSystem>) -> Result<u32, HalError> {
    match system {
        Some(FrontendSystem::IsdbT) => Ok(SYS_ISDBT),
        Some(FrontendSystem::IsdbS) => Ok(SYS_ISDBS),
        Some(FrontendSystem::DvbS) => Ok(SYS_DVBS2),
        Some(FrontendSystem::IsdbS3) => Err(HalError::Unsupported(
            "ISDB-S3 is outside the TS-only product scope",
        )),
        None => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::MissingDeliverySystem,
            "DVB tune request requires a delivery system",
        )),
    }
}

fn validate_stream_id(request: &DvbTuneRequest) -> Result<Option<u16>, HalError> {
    let Some(stream_id) = request.stream_id else {
        return Ok(None);
    };
    if matches!(
        request.stream_id_kind,
        Some(FrontendStreamIdKind::RelativeStreamNumber)
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedStreamSelector,
            "DVB backend does not accept relative stream number",
        ));
    }
    if matches!(request.system, Some(FrontendSystem::IsdbS)) && stream_id < 12 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::InvalidStreamIdRange,
            "BS STREAM_ID must be an absolute TSID",
        ));
    }
    Ok(Some(stream_id))
}

fn validate_symbol_rate(request: &DvbTuneRequest) -> Result<(), HalError> {
    if request.symbol_rate.is_some() {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "r51 DVB backend does not accept explicit symbol_rate",
        ));
    }
    Ok(())
}

fn normalize_bandwidth(request: &DvbTuneRequest) -> Result<Option<u32>, HalError> {
    match request.system {
        Some(FrontendSystem::IsdbT) => match request.bandwidth_hz {
            None | Some(6_000_000) => Ok(Some(6_000_000)),
            Some(_) => Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "r51 DVB ISDB-T accepts only 6MHz bandwidth",
            )),
        },
        Some(FrontendSystem::IsdbS) => match request.bandwidth_hz {
            None => Ok(None),
            Some(_) => Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "r51 DVB ISDB-S does not accept bandwidth_hz",
            )),
        },
        Some(FrontendSystem::IsdbS3 | FrontendSystem::DvbS) | None => Ok(None),
    }
}

pub fn tune_property_pairs(request: &DvbTuneRequest) -> Result<DvbTunePropertyPairs, HalError> {
    validate_symbol_rate(request)?;
    let delivery = delivery_system(request.system)?;
    let bandwidth_hz = normalize_bandwidth(request)?;
    let mut pairs = Vec::new();
    pairs.push((DTV_DELIVERY_SYSTEM, delivery));
    if let Some(freq) = request.frequency_hz {
        pairs.push((DTV_FREQUENCY, freq));
    }
    if let Some(bandwidth_hz) = bandwidth_hz {
        pairs.push((DTV_BANDWIDTH_HZ, bandwidth_hz));
    }
    if let Some(stream_id) = validate_stream_id(request)? {
        pairs.push((DTV_STREAM_ID, u32::from(stream_id)));
    }
    pairs.push((DTV_TUNE, 0));
    Ok(DvbTunePropertyPairs { pairs })
}

fn normalize_stream_id_from_common(
    request: &FrontendTuneRequest,
) -> Result<(Option<u16>, Option<FrontendStreamIdKind>), HalError> {
    let Some(raw_stream_id) = request.stream_id else {
        if matches!(request.system, FrontendSystem::IsdbS) {
            if is_japan_bs_if_frequency_hz(request.frequency) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::MissingStreamSelector,
                    "BS frequency-only tune is rejected",
                ));
            }
            if is_japan_cs110_if_frequency_hz(request.frequency) {
                return Ok((None, None));
            }
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "ISDB-S frequency-only tune is allowed only for CS110",
            ));
        }
        return Ok((None, None));
    };
    let stream_id = u16::try_from(raw_stream_id).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "stream_id is out of u16 range",
        )
    })?;
    match request.stream_id_kind {
        Some(FrontendStreamIdKind::RelativeStreamNumber) => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedStreamSelector,
            "DVB backend rejects relative stream number",
        )),
        Some(FrontendStreamIdKind::AbsoluteStreamId) | None => {
            if matches!(request.system, FrontendSystem::IsdbS) {
                if is_japan_cs110_if_frequency_hz(request.frequency) {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedStreamSelector,
                        "CS110 does not use frontend TSID selection",
                    ));
                }
                if !is_japan_bs_if_frequency_hz(request.frequency) {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedFrequency,
                        "ISDB-S TSID selection is valid only for Japanese BS IF frequencies",
                    ));
                }
                if stream_id < 12 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::InvalidStreamIdRange,
                        "BS STREAM_ID must be an absolute TSID",
                    ));
                }
            }
            Ok((
                Some(stream_id),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
        }
    }
}

pub fn normalized_tune_request_from_common(
    request: &FrontendTuneRequest,
) -> Result<DvbTuneRequest, HalError> {
    let frequency_hz = u32::try_from(request.frequency).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "frequency is out of u32 range",
        )
    })?;
    match request.system {
        FrontendSystem::IsdbT => {
            if !is_japan_isdbt_frequency_contract_hz(request.frequency) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-T frequency is outside the Japanese r51 explicit tune contract",
                ));
            }
        }
        FrontendSystem::IsdbS => {
            if !is_japan_bs_if_frequency_hz(request.frequency)
                && !is_japan_cs110_if_frequency_hz(request.frequency)
            {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency is outside Japanese BS/CS110 IF contract",
                ));
            }
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => {
            return Err(HalError::Unsupported(
                "system is outside r51 DVB explicit tune scope",
            ));
        }
    }
    if request.symbol_rate.is_some() {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "r51 ISDB-T/ISDB-S backend contract rejects explicit symbol_rate",
        ));
    }
    let (stream_id, stream_id_kind) = normalize_stream_id_from_common(request)?;
    let bandwidth_hz = match request.system {
        FrontendSystem::IsdbT => match request.bandwidth_hz {
            None | Some(6_000_000) => Some(6_000_000),
            Some(_) => {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedBandwidth,
                    "r51 DVB ISDB-T accepts only 6MHz bandwidth",
                ))
            }
        },
        FrontendSystem::IsdbS => {
            if request.bandwidth_hz.is_some() {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedBandwidth,
                    "r51 DVB ISDB-S does not accept bandwidth_hz",
                ));
            }
            None
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => None,
    };
    Ok(DvbTuneRequest {
        frequency_hz: Some(frequency_hz),
        stream_id,
        stream_id_kind,
        bandwidth_hz,
        symbol_rate: None,
        system: Some(request.system),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isdbt_properties_include_6mhz_bandwidth() {
        let req = DvbTuneRequest {
            frequency_hz: Some(473_142_857),
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbT),
        };
        let pairs = tune_property_pairs(&req).unwrap().pairs;
        assert_eq!(pairs[0], (DTV_DELIVERY_SYSTEM, SYS_ISDBT));
        assert!(pairs.contains(&(DTV_BANDWIDTH_HZ, 6_000_000)));
        assert_eq!(pairs.last(), Some(&(DTV_TUNE, 0)));
    }

    #[test]
    fn bs_relative_stream_number_is_rejected_on_dvb() {
        let common = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(1),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert!(normalized_tune_request_from_common(&common).is_err());
    }

    #[test]
    fn cs110_frequency_only_is_accepted() {
        let common = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let req = normalized_tune_request_from_common(&common).unwrap();
        assert_eq!(req.stream_id, None);
    }
}
