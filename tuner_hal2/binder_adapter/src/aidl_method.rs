use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AudioStreamType::AudioStreamType, AvStreamType::AvStreamType, DataFormat::DataFormat,
    DvrSettings::DvrSettings, DvrType::DvrType, FilterDelayHint::FilterDelayHint,
    FilterDelayHintType::FilterDelayHintType, LnbPosition::LnbPosition, LnbTone::LnbTone,
    LnbVoltage::LnbVoltage, VideoStreamType::VideoStreamType,
};
#[cfg(test)]
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    PlaybackStatus::PlaybackStatus, RecordStatus::RecordStatus,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_domain_request::{
    DvrConfigureKind, DvrConfigureRequest, DvrDataFormat, DvrOpenKind, FilterAvStreamKind,
    FilterAvStreamTypeRequest, FilterDelayHintKind, FilterDelayHintRequest,
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest, OpenDvrRequest,
};

const DVR_PACKET_SIZE_TS_188: i64 = 188;

fn invalid(detail: &'static str) -> HalError {
    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, detail)
}

pub fn build_dvr_open_request(
    dvr_type: DvrType,
    buffer_size: i32,
) -> Result<OpenDvrRequest, HalError> {
    let kind = match dvr_type {
        DvrType::RECORD => DvrOpenKind::Record,
        DvrType::PLAYBACK => DvrOpenKind::Playback,
        _ => return Err(invalid("DVR type is unsupported")),
    };
    Ok(OpenDvrRequest { kind, buffer_size })
}

pub fn build_filter_av_stream_type_request(
    av_stream_type: &AvStreamType,
) -> Result<FilterAvStreamTypeRequest, HalError> {
    let (kind, stream_type) = match av_stream_type {
        AvStreamType::Video(value) => {
            if !matches!(
                *value,
                VideoStreamType::UNDEFINED
                    | VideoStreamType::RESERVED
                    | VideoStreamType::MPEG1
                    | VideoStreamType::MPEG2
                    | VideoStreamType::MPEG4P2
                    | VideoStreamType::AVC
                    | VideoStreamType::HEVC
                    | VideoStreamType::VC1
                    | VideoStreamType::VP8
                    | VideoStreamType::VP9
                    | VideoStreamType::AV1
                    | VideoStreamType::AVS
                    | VideoStreamType::AVS2
                    | VideoStreamType::VVC
            ) {
                return Err(invalid(
                    "video stream type contains an unknown numeric enum value",
                ));
            }
            (FilterAvStreamKind::Video, value.0)
        }
        AvStreamType::Audio(value) => {
            if !matches!(
                *value,
                AudioStreamType::UNDEFINED
                    | AudioStreamType::PCM
                    | AudioStreamType::MP3
                    | AudioStreamType::MPEG1
                    | AudioStreamType::MPEG2
                    | AudioStreamType::MPEGH
                    | AudioStreamType::AAC
                    | AudioStreamType::AC3
                    | AudioStreamType::EAC3
                    | AudioStreamType::AC4
                    | AudioStreamType::DTS
                    | AudioStreamType::DTS_HD
                    | AudioStreamType::WMA
                    | AudioStreamType::OPUS
                    | AudioStreamType::VORBIS
                    | AudioStreamType::DRA
                    | AudioStreamType::AAC_ADTS
                    | AudioStreamType::AAC_LATM
                    | AudioStreamType::AAC_HE_ADTS
                    | AudioStreamType::AAC_HE_LATM
            ) {
                return Err(invalid(
                    "audio stream type contains an unknown numeric enum value",
                ));
            }
            (FilterAvStreamKind::Audio, value.0)
        }
    };
    Ok(FilterAvStreamTypeRequest { kind, stream_type })
}

pub fn build_filter_delay_hint_request(
    hint: &FilterDelayHint,
) -> Result<FilterDelayHintRequest, HalError> {
    let kind = match hint.hintType {
        FilterDelayHintType::TIME_DELAY_IN_MS => FilterDelayHintKind::TimeDelayMs,
        FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES => FilterDelayHintKind::DataSizeDelayBytes,
        _ => return Err(invalid("filter delay hint type is unsupported")),
    };
    Ok(FilterDelayHintRequest {
        kind,
        value: i64::from(hint.hintValue),
    })
    .and_then(|request| {
        if request.value < 0 {
            return Err(invalid("filter delay hint value must be non-negative"));
        }
        Ok(request)
    })
}

pub fn build_dvr_configure_request(
    settings: &DvrSettings,
) -> Result<DvrConfigureRequest, HalError> {
    match settings {
        DvrSettings::Record(record) => {
            validate_dvr_ts_188(record.dataFormat, record.packetSize)?;
            Ok(DvrConfigureRequest {
                kind: DvrConfigureKind::Record,
                status_mask: record.statusMask,
                low_threshold_bytes: record.lowThreshold,
                high_threshold_bytes: record.highThreshold,
                data_format: DvrDataFormat::Ts,
                packet_size: record.packetSize,
            })
        }
        DvrSettings::Playback(playback) => {
            validate_dvr_ts_188(playback.dataFormat, playback.packetSize)?;
            Ok(DvrConfigureRequest {
                kind: DvrConfigureKind::Playback,
                status_mask: playback.statusMask,
                low_threshold_bytes: playback.lowThreshold,
                high_threshold_bytes: playback.highThreshold,
                data_format: DvrDataFormat::Ts,
                packet_size: playback.packetSize,
            })
        }
    }
}

fn validate_dvr_ts_188(data_format: DataFormat, packet_size: i64) -> Result<(), HalError> {
    match data_format {
        DataFormat::TS => {}
        DataFormat::PES | DataFormat::ES | DataFormat::SHV_TLV => {
            return Err(HalError::unsupported_detail(
                "dvr.dataFormat",
                "known non-TS DVR dataFormat is unavailable in this product profile",
            ));
        }
        DataFormat::UNDEFINED => return Err(invalid("DVR dataFormat must not be UNDEFINED")),
        _ => return Err(invalid("DVR dataFormat contains a reserved enum value")),
    }
    if packet_size <= 0 {
        return Err(invalid("DVR packetSize must be positive"));
    }
    if packet_size != DVR_PACKET_SIZE_TS_188 {
        return Err(HalError::unsupported_detail(
            "dvr.packetSize",
            "positive DVR packetSize other than 188 is unavailable for TS",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn supported_record_status_mask() -> i32 {
    i32::from(RecordStatus::DATA_READY.0)
        | i32::from(RecordStatus::LOW_WATER.0)
        | i32::from(RecordStatus::HIGH_WATER.0)
        | i32::from(RecordStatus::OVERFLOW.0)
}

#[cfg(test)]
fn supported_playback_status_mask() -> i32 {
    i32::from(PlaybackStatus::SPACE_EMPTY.0)
        | i32::from(PlaybackStatus::SPACE_ALMOST_EMPTY.0)
        | i32::from(PlaybackStatus::SPACE_ALMOST_FULL.0)
        | i32::from(PlaybackStatus::SPACE_FULL.0)
}

pub fn build_lnb_voltage_request(voltage: LnbVoltage) -> Result<LnbVoltageRequest, HalError> {
    match voltage {
        LnbVoltage::NONE => Ok(LnbVoltageRequest::None),
        LnbVoltage::VOLTAGE_11V => Ok(LnbVoltageRequest::Voltage11V),
        LnbVoltage::VOLTAGE_15V => Ok(LnbVoltageRequest::Voltage15V),
        LnbVoltage::VOLTAGE_5V
        | LnbVoltage::VOLTAGE_12V
        | LnbVoltage::VOLTAGE_13V
        | LnbVoltage::VOLTAGE_14V
        | LnbVoltage::VOLTAGE_18V
        | LnbVoltage::VOLTAGE_19V => Err(HalError::unsupported_detail(
            "lnb.voltage",
            "known LNB voltage is unavailable in the product profile",
        )),
        _ => Err(invalid("LNB voltage contains a reserved enum value")),
    }
}

pub fn build_lnb_tone_request(tone: LnbTone) -> Result<LnbToneRequest, HalError> {
    match tone {
        LnbTone::NONE => Ok(LnbToneRequest::None),
        LnbTone::CONTINUOUS => Ok(LnbToneRequest::Continuous),
        _ => Err(invalid("LNB tone is unsupported")),
    }
}

pub fn build_lnb_satellite_position_request(
    position: LnbPosition,
) -> Result<LnbSetSatellitePositionRequest, HalError> {
    if !matches!(
        position,
        LnbPosition::UNDEFINED | LnbPosition::POSITION_A | LnbPosition::POSITION_B
    ) {
        return Err(invalid("LNB position contains a reserved enum value"));
    }
    Ok(LnbSetSatellitePositionRequest {
        position: position.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AIDL_TRANSACTION_TABLE;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        PlaybackSettings::PlaybackSettings, RecordSettings::RecordSettings,
    };
    use maleicacid_tuner_hal2_common::{FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest};
    use maleicacid_tuner_hal2_domain_request::{
        AidlMethodAdapter, AidlMethodCall, DvrFilterLinkRequest, FilterReleaseAvHandleRequest,
        FilterSetDataSourceRequest, RuntimeExecutableRequest,
    };

    pub(crate) const AIDL_METHOD_CALL_VARIANT_COUNT_FOR_PLAN_COVERAGE: usize = 46;

    pub(crate) fn all_aidl_method_call_variants_for_plan_coverage(
        request: FrontendTuneRequest,
    ) -> Vec<AidlMethodCall> {
        vec![
            AidlMethodCall::PublicApi {
                object: crate::AidlObjectKind::Tuner,
                api: crate::AidlApi::TunerGetFrontendIds,
            },
            AidlMethodCall::UnsupportedPublicApi {
                object: crate::AidlObjectKind::Tuner,
                api: crate::AidlApi::TunerSetLna,
            },
            AidlMethodCall::FrontendTune(request.clone()),
            AidlMethodCall::FrontendSetLnb { lnb_id: 1 },
            AidlMethodCall::FrontendStopTune,
            AidlMethodCall::FrontendScan(request),
            AidlMethodCall::FrontendStopScan,
            AidlMethodCall::FrontendClose,
            AidlMethodCall::FrontendSetCallback,
            AidlMethodCall::DemuxSetFrontendDataSource { frontend_id: 1 },
            AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                maleicacid_tuner_hal2_demux::config::OpenFilterRequest {
                    open_type: maleicacid_tuner_hal2_demux::config::FilterOpenType::TsSection,
                    buffer_size: 188 * 1024,
                    callback_present: true,
                },
            )),
            AidlMethodCall::DemuxOpenDvr(OpenDvrRequest {
                kind: DvrOpenKind::Record,
                buffer_size: 188 * 1024,
            }),
            AidlMethodCall::DemuxClose,
            AidlMethodCall::FilterConfigure(
                RuntimeExecutableRequest::ConfigureFilterByCurrentOpenType,
            ),
            AidlMethodCall::FilterConfigureAvStreamType(FilterAvStreamTypeRequest {
                kind: FilterAvStreamKind::Video,
                stream_type: 1,
            }),
            AidlMethodCall::FilterGetQueueDesc,
            AidlMethodCall::FilterGetId,
            AidlMethodCall::FilterGetId64Bit,
            AidlMethodCall::FilterGetAvSharedHandle,
            AidlMethodCall::FilterReleaseAvHandle(FilterReleaseAvHandleRequest { av_data_id: 1 }),
            AidlMethodCall::FilterStart,
            AidlMethodCall::FilterStop,
            AidlMethodCall::FilterFlush,
            AidlMethodCall::FilterClose,
            AidlMethodCall::FilterSetDataSource(FilterSetDataSourceRequest {
                source_filter_id: 2,
                source_filter_generation: 1,
            }),
            AidlMethodCall::FilterSetDelayHint(FilterDelayHintRequest {
                kind: FilterDelayHintKind::TimeDelayMs,
                value: 0,
            }),
            AidlMethodCall::DvrGetQueueDesc,
            AidlMethodCall::DvrConfigure(DvrConfigureRequest {
                kind: DvrConfigureKind::Record,
                status_mask: 0,
                low_threshold_bytes: 0,
                high_threshold_bytes: 0,
                data_format: DvrDataFormat::Ts,
                packet_size: DVR_PACKET_SIZE_TS_188,
            }),
            AidlMethodCall::DvrAttachFilter(DvrFilterLinkRequest {
                filter_id: 2,
                filter_generation: 1,
            }),
            AidlMethodCall::DvrDetachFilter(DvrFilterLinkRequest {
                filter_id: 2,
                filter_generation: 1,
            }),
            AidlMethodCall::DvrStart,
            AidlMethodCall::DvrStop,
            AidlMethodCall::DvrFlush,
            AidlMethodCall::DvrClose,
            AidlMethodCall::DvrSetStatusCheckIntervalHint(100),
            AidlMethodCall::DescramblerSetDemuxSource(1),
            AidlMethodCall::DescramblerSetKeyToken(vec![1, 2, 3]),
            AidlMethodCall::DescramblerAddPid(100),
            AidlMethodCall::DescramblerRemovePid(100),
            AidlMethodCall::DescramblerClose,
            AidlMethodCall::LnbSetCallback,
            AidlMethodCall::LnbSetVoltage(LnbVoltageRequest::None),
            AidlMethodCall::LnbSetTone(LnbToneRequest::None),
            AidlMethodCall::LnbSetSatellitePosition(LnbSetSatellitePositionRequest { position: 0 }),
            AidlMethodCall::LnbSendDiseqc(vec![0xe0, 0x10]),
            AidlMethodCall::LnbClose,
        ]
    }

    fn frontend_tune_request_for_test() -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        }
    }

    #[test]
    fn build_dvr_configure_request_keeps_thresholds_and_mask() {
        let request = build_dvr_configure_request(&DvrSettings::Record(RecordSettings {
            statusMask: supported_record_status_mask(),
            lowThreshold: 188,
            highThreshold: 376,
            dataFormat: DataFormat::TS,
            packetSize: 188,
        }))
        .unwrap();
        assert_eq!(
            request,
            DvrConfigureRequest {
                kind: DvrConfigureKind::Record,
                status_mask: supported_record_status_mask(),
                low_threshold_bytes: 188,
                high_threshold_bytes: 376,
                data_format: DvrDataFormat::Ts,
                packet_size: 188,
            }
        );
    }

    #[test]
    fn build_dvr_configure_request_rejects_non_ts_payload_shape() {
        let error = build_dvr_configure_request(&DvrSettings::Playback(PlaybackSettings {
            statusMask: supported_playback_status_mask(),
            lowThreshold: 0,
            highThreshold: 0,
            dataFormat: DataFormat::UNDEFINED,
            packetSize: 192,
        }))
        .unwrap_err();
        assert!(matches!(error, HalError::InvalidArgument { .. }));
    }

    #[test]
    fn build_dvr_configure_request_reports_known_non_ts_format_unavailable() {
        let error = build_dvr_configure_request(&DvrSettings::Playback(PlaybackSettings {
            statusMask: 0,
            lowThreshold: 0,
            highThreshold: 0,
            dataFormat: DataFormat::PES,
            packetSize: 188,
        }))
        .unwrap_err();
        assert!(matches!(error, HalError::UnsupportedDetail { .. }));
    }

    #[test]
    fn build_dvr_configure_request_reports_positive_non_188_packet_unavailable() {
        let error = build_dvr_configure_request(&DvrSettings::Record(RecordSettings {
            statusMask: 0,
            lowThreshold: 0,
            highThreshold: 0,
            dataFormat: DataFormat::TS,
            packetSize: 192,
        }))
        .unwrap_err();
        assert!(matches!(error, HalError::UnsupportedDetail { .. }));
    }

    #[test]
    fn filter_delay_hint_has_no_undocumented_ten_second_cap() {
        let request = build_filter_delay_hint_request(&FilterDelayHint {
            hintType: FilterDelayHintType::TIME_DELAY_IN_MS,
            hintValue: 60_000,
        })
        .unwrap();
        assert_eq!(request.value, 60_000);
    }

    #[test]
    fn av_stream_type_accepts_defined_sentinels_and_codecs_only() {
        for stream_type in [
            VideoStreamType::UNDEFINED,
            VideoStreamType::RESERVED,
            VideoStreamType::MPEG1,
        ] {
            let request =
                build_filter_av_stream_type_request(&AvStreamType::Video(stream_type)).unwrap();
            assert_eq!(request.kind, FilterAvStreamKind::Video);
            assert_eq!(request.stream_type, stream_type.0);
        }
        for stream_type in [
            AudioStreamType::UNDEFINED,
            AudioStreamType::MP3,
            AudioStreamType::MPEG1,
        ] {
            let request =
                build_filter_av_stream_type_request(&AvStreamType::Audio(stream_type)).unwrap();
            assert_eq!(request.kind, FilterAvStreamKind::Audio);
            assert_eq!(request.stream_type, stream_type.0);
        }

        assert!(matches!(
            build_filter_av_stream_type_request(&AvStreamType::Video(VideoStreamType(i32::MAX))),
            Err(HalError::InvalidArgument { .. })
        ));
        assert!(matches!(
            build_filter_av_stream_type_request(&AvStreamType::Audio(AudioStreamType(i32::MAX))),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn known_unavailable_lnb_voltage_is_not_invalid_argument() {
        assert!(matches!(
            build_lnb_voltage_request(LnbVoltage::VOLTAGE_18V),
            Err(HalError::UnsupportedDetail { .. })
        ));
        assert!(matches!(
            build_lnb_voltage_request(LnbVoltage(99)),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn known_lnb_positions_are_retained_but_reserved_values_are_invalid() {
        assert_eq!(
            build_lnb_satellite_position_request(LnbPosition::POSITION_B)
                .unwrap()
                .position,
            LnbPosition::POSITION_B.0
        );
        assert!(matches!(
            build_lnb_satellite_position_request(LnbPosition(99)),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn all_aidl_method_call_variants_have_command_plan_entries() {
        let methods =
            all_aidl_method_call_variants_for_plan_coverage(frontend_tune_request_for_test());
        assert_eq!(
            methods.len(),
            AIDL_METHOD_CALL_VARIANT_COUNT_FOR_PLAN_COVERAGE
        );
        for method in methods {
            let plan = AidlMethodAdapter::plan(method).unwrap();
            assert!(AIDL_TRANSACTION_TABLE.contains(&plan.command_plan));
        }
    }
}
