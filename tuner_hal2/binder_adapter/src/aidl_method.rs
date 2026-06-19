use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AvStreamType::AvStreamType, DvrSettings::DvrSettings, DvrType::DvrType,
    FilterDelayHint::FilterDelayHint, FilterDelayHintType::FilterDelayHintType,
    LnbPosition::LnbPosition, LnbTone::LnbTone, LnbVoltage::LnbVoltage,
};
use maleicacid_tuner_hal2_common::{FrontendTuneRequest, HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_domain_request::{
    DemuxSetFrontendDataSourceRequest, DvrConfigureKind, DvrConfigureRequest, DvrFilterLinkRequest,
    DvrOpenKind, FilterAvStreamKind, FilterAvStreamTypeRequest, FilterDelayHintKind,
    FilterDelayHintRequest, FilterReleaseAvHandleRequest, FilterSetDataSourceRequest,
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest, OpenDvrRequest,
    RuntimeExecutableRequest,
};

use crate::demux::DemuxCommand;
use crate::descrambler::DescramblerCommand;
use crate::dvr::DvrCommand;
use crate::filter::FilterCommand;
use crate::frontend::FrontendCommand;
use crate::lnb::LnbCommand;
use crate::{CommandPlan, DomainCommand};

const MAX_FILTER_DELAY_MS: i64 = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AidlMethodCall {
    PublicApi {
        object: crate::AidlObjectKind,
        api: crate::AidlApi,
    },
    UnsupportedPublicApi {
        object: crate::AidlObjectKind,
        api: crate::AidlApi,
    },
    FrontendTune(FrontendTuneRequest),
    FrontendSetLnb {
        lnb_id: i32,
    },
    FrontendStopTune,
    FrontendScan(FrontendTuneRequest),
    FrontendStopScan,
    FrontendClose,
    FrontendSetCallback,
    DemuxSetFrontendDataSource {
        frontend_id: i32,
    },
    DemuxOpenFilter(RuntimeExecutableRequest),
    DemuxOpenDvr(OpenDvrRequest),
    DemuxClose,
    FilterConfigure(RuntimeExecutableRequest),
    FilterConfigureAvStreamType(FilterAvStreamTypeRequest),
    FilterGetQueueDesc,
    FilterGetId,
    FilterGetId64Bit,
    FilterGetAvSharedHandle,
    FilterReleaseAvHandle(FilterReleaseAvHandleRequest),
    FilterStart,
    FilterStop,
    FilterFlush,
    FilterClose,
    FilterSetDataSource(FilterSetDataSourceRequest),
    FilterSetDelayHint(FilterDelayHintRequest),
    DvrGetQueueDesc,
    DvrConfigure(DvrConfigureRequest),
    DvrAttachFilter(DvrFilterLinkRequest),
    DvrDetachFilter(DvrFilterLinkRequest),
    DvrStart,
    DvrStop,
    DvrFlush,
    DvrClose,
    DvrSetStatusCheckIntervalHint(i64),
    DescramblerSetDemuxSource(i32),
    DescramblerSetKeyToken(Vec<u8>),
    DescramblerAddPid(u16),
    DescramblerRemovePid(u16),
    DescramblerClose,
    LnbSetCallback,
    LnbSetVoltage(LnbVoltageRequest),
    LnbSetTone(LnbToneRequest),
    LnbSetSatellitePosition(LnbSetSatellitePositionRequest),
    LnbSendDiseqc(Vec<u8>),
    LnbClose,
}

impl AidlMethodCall {
    pub const fn api(&self) -> crate::AidlApi {
        match self {
            Self::PublicApi { api, .. } | Self::UnsupportedPublicApi { api, .. } => *api,
            Self::FrontendTune(_) => crate::AidlApi::FrontendTune,
            Self::FrontendSetLnb { .. } => crate::AidlApi::FrontendSetLnb,
            Self::FrontendStopTune => crate::AidlApi::FrontendStopTune,
            Self::FrontendScan(_) => crate::AidlApi::FrontendScan,
            Self::FrontendStopScan => crate::AidlApi::FrontendStopScan,
            Self::FrontendClose => crate::AidlApi::FrontendClose,
            Self::FrontendSetCallback => crate::AidlApi::FrontendSetCallback,
            Self::DemuxSetFrontendDataSource { .. } => crate::AidlApi::DemuxSetFrontendDataSource,
            Self::DemuxOpenFilter(_) => crate::AidlApi::DemuxOpenFilter,
            Self::DemuxOpenDvr(_) => crate::AidlApi::DemuxOpenDvr,
            Self::DemuxClose => crate::AidlApi::DemuxClose,
            Self::FilterConfigure(_) => crate::AidlApi::FilterConfigure,
            Self::FilterConfigureAvStreamType(_) => crate::AidlApi::FilterConfigureAvStreamType,
            Self::FilterGetQueueDesc => crate::AidlApi::FilterGetQueueDesc,
            Self::FilterGetId => crate::AidlApi::FilterGetId,
            Self::FilterGetId64Bit => crate::AidlApi::FilterGetId64Bit,
            Self::FilterGetAvSharedHandle => crate::AidlApi::FilterGetAvSharedHandle,
            Self::FilterReleaseAvHandle(_) => crate::AidlApi::FilterReleaseAvHandle,
            Self::FilterStart => crate::AidlApi::FilterStart,
            Self::FilterStop => crate::AidlApi::FilterStop,
            Self::FilterFlush => crate::AidlApi::FilterFlush,
            Self::FilterClose => crate::AidlApi::FilterClose,
            Self::FilterSetDataSource(_) => crate::AidlApi::FilterSetDataSource,
            Self::FilterSetDelayHint(_) => crate::AidlApi::FilterSetDelayHint,
            Self::DvrGetQueueDesc => crate::AidlApi::DvrGetQueueDesc,
            Self::DvrConfigure(_) => crate::AidlApi::DvrConfigure,
            Self::DvrAttachFilter(_) => crate::AidlApi::DvrAttachFilter,
            Self::DvrDetachFilter(_) => crate::AidlApi::DvrDetachFilter,
            Self::DvrStart => crate::AidlApi::DvrStart,
            Self::DvrStop => crate::AidlApi::DvrStop,
            Self::DvrFlush => crate::AidlApi::DvrFlush,
            Self::DvrClose => crate::AidlApi::DvrClose,
            Self::DvrSetStatusCheckIntervalHint(_) => crate::AidlApi::DvrSetStatusCheckIntervalHint,
            Self::DescramblerSetDemuxSource(_) => crate::AidlApi::DescramblerSetDemuxSource,
            Self::DescramblerSetKeyToken(_) => crate::AidlApi::DescramblerSetKeyToken,
            Self::DescramblerAddPid(_) => crate::AidlApi::DescramblerAddPid,
            Self::DescramblerRemovePid(_) => crate::AidlApi::DescramblerRemovePid,
            Self::DescramblerClose => crate::AidlApi::DescramblerClose,
            Self::LnbSetCallback => crate::AidlApi::LnbSetCallback,
            Self::LnbSetVoltage(_) => crate::AidlApi::LnbSetVoltage,
            Self::LnbSetTone(_) => crate::AidlApi::LnbSetTone,
            Self::LnbSetSatellitePosition(_) => crate::AidlApi::LnbSetSatellitePosition,
            Self::LnbSendDiseqc(_) => crate::AidlApi::LnbSendDiseqc,
            Self::LnbClose => crate::AidlApi::LnbClose,
        }
    }

    pub fn into_domain_command(self) -> DomainCommand {
        match self {
            Self::PublicApi { object, api } => DomainCommand::PublicApi { object, api },
            Self::UnsupportedPublicApi { object, api } => DomainCommand::UnsupportedPublicApi {
                object,
                api,
                request: None,
            },
            Self::FrontendTune(request) => DomainCommand::Frontend(FrontendCommand::Tune(request)),
            Self::FrontendSetLnb { lnb_id } => {
                DomainCommand::Frontend(FrontendCommand::SetLnb(lnb_id))
            }
            Self::FrontendStopTune => DomainCommand::Frontend(FrontendCommand::StopTune),
            Self::FrontendScan(request) => DomainCommand::Frontend(FrontendCommand::Scan(request)),
            Self::FrontendStopScan => DomainCommand::Frontend(FrontendCommand::StopScan),
            Self::FrontendClose => DomainCommand::Frontend(FrontendCommand::Close),
            Self::FrontendSetCallback => DomainCommand::Frontend(FrontendCommand::SetCallback(
                RuntimeExecutableRequest::NoPayload,
            )),
            Self::DemuxSetFrontendDataSource { frontend_id } => {
                DomainCommand::Demux(DemuxCommand::SetFrontendDataSource(
                    RuntimeExecutableRequest::DemuxSetFrontendDataSource(
                        DemuxSetFrontendDataSourceRequest { frontend_id },
                    ),
                ))
            }
            Self::DemuxOpenFilter(request) => {
                DomainCommand::Demux(DemuxCommand::OpenFilter(request))
            }
            Self::DemuxOpenDvr(request) => DomainCommand::Demux(DemuxCommand::OpenDvr(
                RuntimeExecutableRequest::OpenDvr(request),
            )),
            Self::DemuxClose => DomainCommand::Demux(DemuxCommand::Close),
            Self::FilterConfigure(request) => {
                DomainCommand::Filter(FilterCommand::Configure(request))
            }
            Self::FilterConfigureAvStreamType(request) => {
                DomainCommand::Filter(FilterCommand::ConfigureAvStreamType(
                    RuntimeExecutableRequest::FilterConfigureAvStreamType(request),
                ))
            }
            Self::FilterGetQueueDesc => DomainCommand::Filter(FilterCommand::GetQueueDesc),
            Self::FilterGetId => DomainCommand::Filter(FilterCommand::GetId),
            Self::FilterGetId64Bit => DomainCommand::Filter(FilterCommand::GetId64Bit),
            Self::FilterGetAvSharedHandle => {
                DomainCommand::Filter(FilterCommand::GetAvSharedHandle)
            }
            Self::FilterReleaseAvHandle(request) => {
                DomainCommand::Filter(FilterCommand::ReleaseAvHandle(
                    RuntimeExecutableRequest::FilterReleaseAvHandle(request),
                ))
            }
            Self::FilterStart => DomainCommand::Filter(FilterCommand::Start),
            Self::FilterStop => DomainCommand::Filter(FilterCommand::Stop),
            Self::FilterFlush => DomainCommand::Filter(FilterCommand::Flush),
            Self::FilterClose => DomainCommand::Filter(FilterCommand::Close),
            Self::FilterSetDataSource(request) => {
                DomainCommand::Filter(FilterCommand::SetDataSource(
                    RuntimeExecutableRequest::FilterSetDataSource(request),
                ))
            }
            Self::FilterSetDelayHint(request) => DomainCommand::Filter(
                FilterCommand::SetDelayHint(RuntimeExecutableRequest::FilterDelayHint(request)),
            ),
            Self::DvrGetQueueDesc => DomainCommand::Dvr(DvrCommand::GetQueueDesc),
            Self::DvrConfigure(request) => DomainCommand::Dvr(DvrCommand::Configure(
                RuntimeExecutableRequest::DvrConfigure(request),
            )),
            Self::DvrAttachFilter(request) => DomainCommand::Dvr(DvrCommand::AttachFilter(
                RuntimeExecutableRequest::DvrAttachFilter(request),
            )),
            Self::DvrDetachFilter(request) => DomainCommand::Dvr(DvrCommand::DetachFilter(
                RuntimeExecutableRequest::DvrDetachFilter(request),
            )),
            Self::DvrStart => DomainCommand::Dvr(DvrCommand::Start),
            Self::DvrStop => DomainCommand::Dvr(DvrCommand::Stop),
            Self::DvrFlush => DomainCommand::Dvr(DvrCommand::Flush),
            Self::DvrClose => DomainCommand::Dvr(DvrCommand::Close),
            Self::DvrSetStatusCheckIntervalHint(v) => {
                DomainCommand::Dvr(DvrCommand::SetStatusCheckIntervalHint(v))
            }
            Self::DescramblerSetDemuxSource(v) => {
                DomainCommand::Descrambler(DescramblerCommand::SetDemuxSource(v))
            }
            Self::DescramblerSetKeyToken(v) => {
                DomainCommand::Descrambler(DescramblerCommand::SetKeyToken(v))
            }
            Self::DescramblerAddPid(v) => DomainCommand::Descrambler(DescramblerCommand::AddPid(v)),
            Self::DescramblerRemovePid(v) => {
                DomainCommand::Descrambler(DescramblerCommand::RemovePid(v))
            }
            Self::DescramblerClose => DomainCommand::Descrambler(DescramblerCommand::Close),
            Self::LnbSetCallback => {
                DomainCommand::Lnb(LnbCommand::SetCallback(RuntimeExecutableRequest::NoPayload))
            }
            Self::LnbSetVoltage(request) => DomainCommand::Lnb(LnbCommand::SetVoltage(
                RuntimeExecutableRequest::LnbSetVoltage(request),
            )),
            Self::LnbSetTone(request) => DomainCommand::Lnb(LnbCommand::SetTone(
                RuntimeExecutableRequest::LnbSetTone(request),
            )),
            Self::LnbSetSatellitePosition(request) => {
                DomainCommand::Lnb(LnbCommand::SetSatellitePosition(
                    RuntimeExecutableRequest::LnbSetSatellitePosition(request),
                ))
            }
            Self::LnbSendDiseqc(v) => DomainCommand::Lnb(LnbCommand::SendDiseqc(v)),
            Self::LnbClose => DomainCommand::Lnb(LnbCommand::Close),
        }
    }
}

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
        AvStreamType::Video(value) => (FilterAvStreamKind::Video, value.0),
        AvStreamType::Audio(value) => (FilterAvStreamKind::Audio, value.0),
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
        if request.kind == FilterDelayHintKind::TimeDelayMs && request.value > MAX_FILTER_DELAY_MS {
            return Err(invalid("filter delay time hint exceeds product limit"));
        }
        Ok(request)
    })
}

pub fn build_dvr_configure_request(
    settings: &DvrSettings,
) -> Result<DvrConfigureRequest, HalError> {
    let kind = match settings {
        DvrSettings::Record(_) => DvrConfigureKind::Record,
        DvrSettings::Playback(_) => DvrConfigureKind::Playback,
    };
    Ok(DvrConfigureRequest { kind })
}

pub fn build_lnb_voltage_request(voltage: LnbVoltage) -> Result<LnbVoltageRequest, HalError> {
    match voltage {
        LnbVoltage::NONE => Ok(LnbVoltageRequest::None),
        LnbVoltage::VOLTAGE_11V => Ok(LnbVoltageRequest::Voltage11V),
        LnbVoltage::VOLTAGE_15V => Ok(LnbVoltageRequest::Voltage15V),
        _ => Err(invalid("LNB voltage is unsupported")),
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
    Ok(LnbSetSatellitePositionRequest {
        position: position.0,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AidlMethodPlan {
    pub api: crate::AidlApi,
    pub command: DomainCommand,
    pub command_plan: CommandPlan,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AidlMethodAdapter;

impl AidlMethodAdapter {
    pub fn plan(method: AidlMethodCall) -> Result<AidlMethodPlan, HalError> {
        let api = method.api();
        let command = method.into_domain_command();
        let command_plan = command.plan()?;
        Ok(AidlMethodPlan {
            api,
            command,
            command_plan,
        })
    }

    pub fn frontend_tune(request: FrontendTuneRequest) -> Result<AidlMethodPlan, HalError> {
        Self::plan(AidlMethodCall::FrontendTune(request))
    }
}

#[cfg(test)]
pub(crate) use tests::{
    all_aidl_method_call_variants_for_plan_coverage,
    AIDL_METHOD_CALL_VARIANT_COUNT_FOR_PLAN_COVERAGE,
};

#[cfg(test)]
mod tests {
    use super::*;

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
            AidlMethodCall::FilterConfigure(RuntimeExecutableRequest::ConfigureFilterByCurrentOpenType),
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
}
