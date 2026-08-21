use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidArgumentKind};

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
    TunerPublicApiTxn,
    FrontendPublicApiTxn,
    DemuxPublicApiTxn,
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
    object: AidlObjectKind,
    api: AidlApi,
    transaction: RuntimeTransactionName,
}

impl CommandPlan {
    const fn table_entry(
        object: AidlObjectKind,
        api: AidlApi,
        transaction: RuntimeTransactionName,
    ) -> Self {
        Self {
            object,
            api,
            transaction,
        }
    }

    pub const fn object(self) -> AidlObjectKind {
        self.object
    }

    pub const fn api(self) -> AidlApi {
        self.api
    }

    pub const fn transaction(self) -> RuntimeTransactionName {
        self.transaction
    }

    pub fn for_api(object: AidlObjectKind, api: AidlApi) -> Result<Self, HalError> {
        AIDL_TRANSACTION_TABLE
            .iter()
            .copied()
            .find(|plan| plan.object == object && plan.api == api)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "AIDL command plan is missing from the transaction table",
                )
            })
    }
}

pub const AIDL_TRANSACTION_TABLE: &[CommandPlan] = &[
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetFrontendIds,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenFrontendById,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenDemux,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetDemuxCaps,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenDescrambler,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetFrontendInfo,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenLnbById,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenLnbByName,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetLnbIds,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerSetLna,
        RuntimeTransactionName::TunerUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerSetMaxNumberOfFrontends,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetMaxNumberOfFrontends,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerIsLnaSupported,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetDemuxIds,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerOpenDemuxById,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Tuner,
        AidlApi::TunerGetDemuxInfo,
        RuntimeTransactionName::TunerPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendGetStatus,
        RuntimeTransactionName::FrontendPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendSetLnb,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendLinkCiCam,
        RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendUnlinkCiCam,
        RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendGetHardwareInfo,
        RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendRemoveOutputPid,
        RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendGetFrontendStatusReadiness,
        RuntimeTransactionName::FrontendPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxOpenTimeFilter,
        RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxGetAvSyncHwId,
        RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxGetAvSyncTime,
        RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxConnectCiCam,
        RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxDisconnectCiCam,
        RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendTune,
        RuntimeTransactionName::FrontendTuneTxnApply,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendStopTune,
        RuntimeTransactionName::FrontendStopTuneTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendScan,
        RuntimeTransactionName::FrontendScanTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendStopScan,
        RuntimeTransactionName::FrontendStopScanTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendClose,
        RuntimeTransactionName::FrontendCloseLifecycleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Frontend,
        AidlApi::FrontendSetCallback,
        RuntimeTransactionName::FrontendCallbackRegistrationTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxSetFrontendDataSource,
        RuntimeTransactionName::DemuxSetFrontendDataSourceTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxOpenFilter,
        RuntimeTransactionName::DemuxOpenFilterTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxOpenDvr,
        RuntimeTransactionName::DemuxOpenDvrTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Demux,
        AidlApi::DemuxClose,
        RuntimeTransactionName::DemuxCloseLifecycleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterConfigure,
        RuntimeTransactionName::FilterConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterConfigureAvStreamType,
        RuntimeTransactionName::FilterConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterGetQueueDesc,
        RuntimeTransactionName::FilterGetQueueDescTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterGetId,
        RuntimeTransactionName::FilterGetIdTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterGetId64Bit,
        RuntimeTransactionName::FilterGetId64BitTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterGetAvSharedHandle,
        RuntimeTransactionName::FilterGetAvSharedHandleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterReleaseAvHandle,
        RuntimeTransactionName::FilterReleaseAvHandleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterStart,
        RuntimeTransactionName::FilterStartTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterStop,
        RuntimeTransactionName::FilterStopTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterFlush,
        RuntimeTransactionName::FilterFlushTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterClose,
        RuntimeTransactionName::FilterCloseLifecycleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterSetDataSource,
        RuntimeTransactionName::FilterSetDataSourceTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Filter,
        AidlApi::FilterSetDelayHint,
        RuntimeTransactionName::FilterConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrGetQueueDesc,
        RuntimeTransactionName::DvrGetQueueDescTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrConfigure,
        RuntimeTransactionName::DvrConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrAttachFilter,
        RuntimeTransactionName::DvrConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrDetachFilter,
        RuntimeTransactionName::DvrConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrStart,
        RuntimeTransactionName::DvrStartTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrStop,
        RuntimeTransactionName::DvrStopTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrFlush,
        RuntimeTransactionName::DvrFlushTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrClose,
        RuntimeTransactionName::DvrCloseLifecycleTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Dvr,
        AidlApi::DvrSetStatusCheckIntervalHint,
        RuntimeTransactionName::DvrConfigureTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Descrambler,
        AidlApi::DescramblerSetDemuxSource,
        RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Descrambler,
        AidlApi::DescramblerSetKeyToken,
        RuntimeTransactionName::DescramblerSessionTxnSetKeyToken,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Descrambler,
        AidlApi::DescramblerAddPid,
        RuntimeTransactionName::DescramblerSessionTxnAddPid,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Descrambler,
        AidlApi::DescramblerRemovePid,
        RuntimeTransactionName::DescramblerSessionTxnRemovePid,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Descrambler,
        AidlApi::DescramblerClose,
        RuntimeTransactionName::DescramblerSessionTxnClose,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbSetCallback,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbSetVoltage,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbSetTone,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbSetSatellitePosition,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbSendDiseqc,
        RuntimeTransactionName::LnbApplyTxn,
    ),
    CommandPlan::table_entry(
        AidlObjectKind::Lnb,
        AidlApi::LnbClose,
        RuntimeTransactionName::LnbLifecycleTxnClose,
    ),
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
pub enum DvrDataFormat {
    Ts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrConfigureRequest {
    pub kind: DvrConfigureKind,
    pub status_mask: i32,
    pub low_threshold_bytes: i64,
    pub high_threshold_bytes: i64,
    pub data_format: DvrDataFormat,
    pub packet_size: i64,
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
    ConfigureFilterByCurrentOpenType,
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
            Self::UnsupportedProfile { .. } => {
                DomainProfileSupport::UnsupportedRecordThenUnavailable
            }
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
            | Self::ConfigureFilterByCurrentOpenType
            | Self::UnsupportedProfile { .. }
            | Self::FilterSetDataSource(_)
            | Self::DvrConfigure(_)
            | Self::LnbSetVoltage(_)
            | Self::LnbSetTone(_) => Ok(()),
        }
    }
}
