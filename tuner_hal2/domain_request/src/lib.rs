use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};



#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AidlObjectKind {
    Tuner,
    Frontend,
    Demux,
    Filter,
    Dvr,
    Descrambler,
    Lnb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AidlObjectId(pub i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AidlObjectGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AidlApi {
    TunerGetFrontendIds,
    TunerOpenFrontendById,
    TunerOpenDemux,
    TunerGetDemuxCaps,
    TunerOpenDescrambler,
    TunerGetFrontendInfo,
    TunerOpenLnbById,
    TunerOpenLnbByName,
    TunerGetLnbIds,
    TunerSetLna,
    TunerSetMaxNumberOfFrontends,
    TunerGetMaxNumberOfFrontends,
    TunerIsLnaSupported,
    TunerGetDemuxIds,
    TunerOpenDemuxById,
    TunerGetDemuxInfo,
    FrontendGetStatus,
    FrontendSetLnb,
    FrontendLinkCiCam,
    FrontendUnlinkCiCam,
    FrontendGetHardwareInfo,
    FrontendRemoveOutputPid,
    FrontendGetFrontendStatusReadiness,
    DemuxOpenTimeFilter,
    DemuxGetAvSyncHwId,
    DemuxGetAvSyncTime,
    DemuxConnectCiCam,
    DemuxDisconnectCiCam,
    FrontendTune,
    FrontendStopTune,
    FrontendScan,
    FrontendStopScan,
    FrontendClose,
    FrontendSetCallback,
    DemuxSetFrontendDataSource,
    DemuxOpenFilter,
    DemuxOpenDvr,
    DemuxClose,
    FilterConfigure,
    FilterConfigureAvStreamType,
    FilterGetQueueDesc,
    FilterGetId,
    FilterGetId64Bit,
    FilterGetAvSharedHandle,
    FilterReleaseAvHandle,
    FilterStart,
    FilterStop,
    FilterFlush,
    FilterClose,
    FilterSetDataSource,
    FilterSetDelayHint,
    DvrGetQueueDesc,
    DvrConfigure,
    DvrAttachFilter,
    DvrDetachFilter,
    DvrStart,
    DvrStop,
    DvrFlush,
    DvrClose,
    DvrSetStatusCheckIntervalHint,
    DescramblerSetDemuxSource,
    DescramblerSetKeyToken,
    DescramblerAddPid,
    DescramblerRemovePid,
    DescramblerClose,
    LnbSetCallback,
    LnbSetVoltage,
    LnbSetTone,
    LnbSetSatellitePosition,
    LnbSendDiseqc,
    LnbClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeTransactionName {
    TunerUnsupportedPublicApiTxn,
    FrontendUnsupportedPublicApiTxn,
    DemuxUnsupportedPublicApiTxn,
    FrontendTuneTxnApply,
    FrontendStopTuneTxn,
    FrontendScanTxn,
    FrontendStopScanTxn,
    FrontendCloseLifecycleTxn,
    FrontendCallbackRegistrationTxn,
    DemuxSetFrontendDataSourceTxn,
    DemuxOpenFilterTxn,
    DemuxOpenDvrTxn,
    DemuxCloseLifecycleTxn,
    FilterConfigureTxn,
    FilterGetQueueDescTxn,
    FilterGetIdTxn,
    FilterGetId64BitTxn,
    FilterGetAvSharedHandleTxn,
    FilterReleaseAvHandleTxn,
    FilterStartTxn,
    FilterStopTxn,
    FilterFlushTxn,
    FilterCloseLifecycleTxn,
    FilterSetDataSourceTxn,
    DvrGetQueueDescTxn,
    DvrConfigureTxn,
    DvrStartTxn,
    DvrStopTxn,
    DvrFlushTxn,
    DvrCloseLifecycleTxn,
    DescramblerSessionTxnSetDemuxSource,
    DescramblerSessionTxnSetKeyToken,
    DescramblerSessionTxnAddPid,
    DescramblerSessionTxnRemovePid,
    DescramblerSessionTxnClose,
    LnbApplyTxn,
    LnbLifecycleTxnClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandPlan {
    pub object: AidlObjectKind,
    pub api: AidlApi,
    pub transaction: RuntimeTransactionName,
}

pub const AIDL_TRANSACTION_TABLE: &[CommandPlan] = &[
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetFrontendIds, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenFrontendById, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenDemux, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetDemuxCaps, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenDescrambler, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetFrontendInfo, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenLnbById, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenLnbByName, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetLnbIds, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerSetLna, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerSetMaxNumberOfFrontends, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetMaxNumberOfFrontends, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerIsLnaSupported, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetDemuxIds, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerOpenDemuxById, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Tuner, api: AidlApi::TunerGetDemuxInfo, transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendGetStatus, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendSetLnb, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendLinkCiCam, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendUnlinkCiCam, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendGetHardwareInfo, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendRemoveOutputPid, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendGetFrontendStatusReadiness, transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxOpenTimeFilter, transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxGetAvSyncHwId, transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxGetAvSyncTime, transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxConnectCiCam, transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxDisconnectCiCam, transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendTune, transaction: RuntimeTransactionName::FrontendTuneTxnApply },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendStopTune, transaction: RuntimeTransactionName::FrontendStopTuneTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendScan, transaction: RuntimeTransactionName::FrontendScanTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendStopScan, transaction: RuntimeTransactionName::FrontendStopScanTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendClose, transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn },
    CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendSetCallback, transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxSetFrontendDataSource, transaction: RuntimeTransactionName::DemuxSetFrontendDataSourceTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxOpenFilter, transaction: RuntimeTransactionName::DemuxOpenFilterTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxOpenDvr, transaction: RuntimeTransactionName::DemuxOpenDvrTxn },
    CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxClose, transaction: RuntimeTransactionName::DemuxCloseLifecycleTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterConfigure, transaction: RuntimeTransactionName::FilterConfigureTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterConfigureAvStreamType, transaction: RuntimeTransactionName::FilterConfigureTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetQueueDesc, transaction: RuntimeTransactionName::FilterGetQueueDescTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetId, transaction: RuntimeTransactionName::FilterGetIdTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetId64Bit, transaction: RuntimeTransactionName::FilterGetId64BitTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetAvSharedHandle, transaction: RuntimeTransactionName::FilterGetAvSharedHandleTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterReleaseAvHandle, transaction: RuntimeTransactionName::FilterReleaseAvHandleTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterStart, transaction: RuntimeTransactionName::FilterStartTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterStop, transaction: RuntimeTransactionName::FilterStopTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterFlush, transaction: RuntimeTransactionName::FilterFlushTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterClose, transaction: RuntimeTransactionName::FilterCloseLifecycleTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterSetDataSource, transaction: RuntimeTransactionName::FilterSetDataSourceTxn },
    CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterSetDelayHint, transaction: RuntimeTransactionName::FilterConfigureTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrGetQueueDesc, transaction: RuntimeTransactionName::DvrGetQueueDescTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrConfigure, transaction: RuntimeTransactionName::DvrConfigureTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrAttachFilter, transaction: RuntimeTransactionName::DvrConfigureTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrDetachFilter, transaction: RuntimeTransactionName::DvrConfigureTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrStart, transaction: RuntimeTransactionName::DvrStartTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrStop, transaction: RuntimeTransactionName::DvrStopTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrFlush, transaction: RuntimeTransactionName::DvrFlushTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrClose, transaction: RuntimeTransactionName::DvrCloseLifecycleTxn },
    CommandPlan { object: AidlObjectKind::Dvr, api: AidlApi::DvrSetStatusCheckIntervalHint, transaction: RuntimeTransactionName::DvrConfigureTxn },
    CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerSetDemuxSource, transaction: RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource },
    CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerSetKeyToken, transaction: RuntimeTransactionName::DescramblerSessionTxnSetKeyToken },
    CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerAddPid, transaction: RuntimeTransactionName::DescramblerSessionTxnAddPid },
    CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerRemovePid, transaction: RuntimeTransactionName::DescramblerSessionTxnRemovePid },
    CommandPlan { object: AidlObjectKind::Descrambler, api: AidlApi::DescramblerClose, transaction: RuntimeTransactionName::DescramblerSessionTxnClose },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetCallback, transaction: RuntimeTransactionName::LnbApplyTxn },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetVoltage, transaction: RuntimeTransactionName::LnbApplyTxn },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetTone, transaction: RuntimeTransactionName::LnbApplyTxn },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetSatellitePosition, transaction: RuntimeTransactionName::LnbApplyTxn },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSendDiseqc, transaction: RuntimeTransactionName::LnbApplyTxn },
    CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbClose, transaction: RuntimeTransactionName::LnbLifecycleTxnClose },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainProfileSupport {
    Supported,
    UnsupportedRecordThenUnavailable,
}

use maleicacid_tuner_hal2_demux::config::{FilterConfig, OpenFilterRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxSetFrontendDataSourceRequest {
    pub frontend_id: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrOpenKind {
    Record,
    Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenDvrRequest {
    pub kind: DvrOpenKind,
    pub buffer_size: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterAvStreamKind {
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterAvStreamTypeRequest {
    pub kind: FilterAvStreamKind,
    pub stream_type: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterReleaseAvHandleRequest {
    pub av_data_id: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterSetDataSourceRequest {
    pub source_filter_id: i64,
    pub source_filter_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDelayHintKind {
    TimeDelayMs,
    DataSizeDelayBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterDelayHintRequest {
    pub kind: FilterDelayHintKind,
    pub value: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrConfigureKind {
    Record,
    Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrConfigureRequest {
    pub kind: DvrConfigureKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrFilterLinkRequest {
    pub filter_id: i64,
    pub filter_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbVoltageRequest {
    None,
    Voltage11V,
    Voltage15V,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbToneRequest {
    None,
    Continuous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LnbSetSatellitePositionRequest {
    pub position: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeExecutableRequest {
    NoPayload,
    OpenFilter(OpenFilterRequest),
    ConfigureFilter(FilterConfig),
    UnsupportedProfile { reason: &'static str },
    DemuxSetFrontendDataSource(DemuxSetFrontendDataSourceRequest),
    OpenDvr(OpenDvrRequest),
    FilterConfigureAvStreamType(FilterAvStreamTypeRequest),
    FilterReleaseAvHandle(FilterReleaseAvHandleRequest),
    FilterSetDataSource(FilterSetDataSourceRequest),
    FilterDelayHint(FilterDelayHintRequest),
    DvrConfigure(DvrConfigureRequest),
    DvrAttachFilter(DvrFilterLinkRequest),
    DvrDetachFilter(DvrFilterLinkRequest),
    LnbSetVoltage(LnbVoltageRequest),
    LnbSetTone(LnbToneRequest),
    LnbSetSatellitePosition(LnbSetSatellitePositionRequest),
}

pub type AidlDomainRequest = RuntimeExecutableRequest;

impl RuntimeExecutableRequest {
    pub fn profile_support(&self) -> DomainProfileSupport {
        match self {
            Self::UnsupportedProfile { .. } => DomainProfileSupport::UnsupportedRecordThenUnavailable,
            _ => DomainProfileSupport::Supported,
        }
    }

    pub fn validate_supported_values(&self) -> Result<(), HalError> {
        match self {
            Self::OpenFilter(request) => {
                if request.buffer_size <= 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter buffer size must be positive",
                    ));
                }
                Ok(())
            }
            Self::OpenDvr(request) => {
                if request.buffer_size <= 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "DVR buffer size must be positive",
                    ));
                }
                Ok(())
            }
            Self::DemuxSetFrontendDataSource(request) => {
                if request.frontend_id < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "frontend id must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::FilterConfigureAvStreamType(request) => {
                if request.stream_type < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "AV stream type must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::FilterReleaseAvHandle(request) => {
                if request.av_data_id < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "AV data id must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::FilterDelayHint(request) => {
                if request.value < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay hint value must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::DvrAttachFilter(request) | Self::DvrDetachFilter(request) => {
                if request.filter_id < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "DVR filter id must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::LnbSetSatellitePosition(request) => {
                if request.position < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "LNB satellite position must be non-negative",
                    ));
                }
                Ok(())
            }
            Self::NoPayload
            | Self::ConfigureFilter(_)
            | Self::UnsupportedProfile { .. }
            | Self::FilterSetDataSource(_)
            | Self::DvrConfigure(_)
            | Self::LnbSetVoltage(_)
            | Self::LnbSetTone(_) => Ok(()),
        }
    }
}
