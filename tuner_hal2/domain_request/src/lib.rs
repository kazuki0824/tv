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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainValueValidation {
    Bool,
    I32Any,
    I32NonNegative,
    I32Positive,
    I64Any,
    I64NonNegative,
    U16TsPid,
    EnumKnown,
    HandlePresence,
    BytesLength,
    DebugOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainRequestField {
    pub name: &'static str,
    pub value: String,
    pub validation: DomainValueValidation,
}

impl DomainRequestField {
    pub fn new(name: &'static str, value: impl Into<String>, validation: DomainValueValidation) -> Self {
        Self { name, value: value.into(), validation }
    }

    pub fn validate(&self) -> Result<(), HalError> {
        match self.validation {
            DomainValueValidation::Bool => match self.value.as_str() {
                "true" | "false" => Ok(()),
                _ => Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be bool", self.name))),
            },
            DomainValueValidation::I32Any | DomainValueValidation::EnumKnown | DomainValueValidation::DebugOnly | DomainValueValidation::HandlePresence | DomainValueValidation::BytesLength => Ok(()),
            DomainValueValidation::I32NonNegative => {
                let value = self.value.parse::<i64>().map_err(|_| HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be integer", self.name)))?;
                if value >= 0 { Ok(()) } else { Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be non-negative", self.name))) }
            }
            DomainValueValidation::I32Positive => {
                let value = self.value.parse::<i64>().map_err(|_| HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be integer", self.name)))?;
                if value > 0 { Ok(()) } else { Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be positive", self.name))) }
            }
            DomainValueValidation::I64Any => Ok(()),
            DomainValueValidation::I64NonNegative => {
                let value = self.value.parse::<i128>().map_err(|_| HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be integer", self.name)))?;
                if value >= 0 { Ok(()) } else { Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be non-negative", self.name))) }
            }
            DomainValueValidation::U16TsPid => {
                let value = self.value.parse::<i64>().map_err(|_| HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be integer", self.name)))?;
                if (0..=0x1ffe).contains(&value) { Ok(()) } else { Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be TS PID 0..=0x1ffe", self.name))) }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TsPid(pub u16);

impl TsPid {
    pub fn parse(name: &'static str, value: &str) -> Result<Self, HalError> {
        let pid = value.parse::<i64>().map_err(|_| HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be integer", name)))?;
        if (0..=0x1ffe).contains(&pid) {
            Ok(Self(pid as u16))
        } else {
            Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("{} must be TS PID 0..=0x1ffe", name)))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxFilterRootVariant { Ts, Mmtp, Ip, Tlv, Alp, Unknown }

impl DemuxFilterRootVariant {
    pub fn profile_support(self) -> DomainProfileSupport {
        match self {
            DemuxFilterRootVariant::Ts => DomainProfileSupport::Supported,
            DemuxFilterRootVariant::Mmtp | DemuxFilterRootVariant::Ip | DemuxFilterRootVariant::Tlv | DemuxFilterRootVariant::Alp | DemuxFilterRootVariant::Unknown => DomainProfileSupport::UnsupportedRecordThenUnavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TsFilterSubVariant { Noinit, Section, Av, PesData, Record, Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxFilterTypeDomain { Ts, Section, Av, PesData, Record, Unknown }

impl DemuxFilterTypeDomain {
    pub fn is_compatible_with_ts_variant(self, variant: TsFilterSubVariant) -> bool {
        match (self, variant) {
            (Self::Ts, TsFilterSubVariant::Noinit) => true,
            (Self::Section, TsFilterSubVariant::Section) => true,
            (Self::Av, TsFilterSubVariant::Av) => true,
            (Self::PesData, TsFilterSubVariant::PesData) => true,
            (Self::Record, TsFilterSubVariant::Record) => true,
            (Self::Unknown, _) | (_, TsFilterSubVariant::Unknown) => false,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionConditionKind { SectionBits, TableInfo, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SectionBitsConditionRequest {
    pub filter: Vec<u8>,
    pub mask: Vec<u8>,
    pub mode: Vec<u8>,
}

impl SectionBitsConditionRequest {
    pub fn validate_lengths(&self) -> Result<(), HalError> {
        let filter_len = self.filter.len();
        if self.mask.len() != filter_len || self.mode.len() != filter_len {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "section condition filter/mask/mode byte arrays must have identical lengths",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableInfoConditionRequest {
    pub table_id: i32,
    pub version: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SectionConditionRequest {
    SectionBits(SectionBitsConditionRequest),
    TableInfo(TableInfoConditionRequest),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TsNoinitFilterRequest {
    pub tpid: TsPid,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TsSectionFilterRequest {
    pub tpid: TsPid,
    pub is_check_crc: bool,
    pub is_repeat: bool,
    pub is_raw: bool,
    pub bit_width_of_length_field: i32,
    pub condition: SectionConditionRequest,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TsAvFilterRequest {
    pub tpid: TsPid,
    pub is_passthrough: bool,
    pub is_secure_memory: bool,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TsPesDataFilterRequest {
    pub tpid: TsPid,
    pub stream_id: i32,
    pub is_raw: bool,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScIndexMaskRequest {
    pub raw_debug: Option<String>,
    pub normalized_mask: Option<i32>,
}

impl ScIndexMaskRequest {
    pub fn from_debug(raw_debug: Option<String>) -> Self {
        let normalized_mask = raw_debug
            .as_deref()
            .and_then(|value| value.trim().parse::<i32>().ok());
        Self { raw_debug, normalized_mask }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TsRecordFilterRequest {
    pub tpid: TsPid,
    pub ts_index_mask: i32,
    pub sc_index_type: i32,
    pub sc_index_mask: ScIndexMaskRequest,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxOpenFilterRequest {
    pub filter_type: DemuxFilterTypeDomain,
    pub buffer_size: i32,
    pub callback_present: bool,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxOpenDvrRequest {
    pub direction: DvrDirection,
    pub buffer_size: i32,
    pub callback_present: bool,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedDemuxFilterRequest {
    pub root_variant: DemuxFilterRootVariant,
    pub reason: &'static str,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TsFilterRuntimeRequest {
    Noinit(TsNoinitFilterRequest),
    Section(TsSectionFilterRequest),
    Av(TsAvFilterRequest),
    PesData(TsPesDataFilterRequest),
    Record(TsRecordFilterRequest),
    UnsupportedTsVariant(UnsupportedDemuxFilterRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DemuxFilterDomainRequest {
    Ts(TsFilterRuntimeRequest),
    UnsupportedVariant(UnsupportedDemuxFilterRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrDirection { Record, Playback, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrRuntimeRequest {
    pub direction: DvrDirection,
    pub packet_size: i64,
    pub low_threshold: i64,
    pub high_threshold: i64,
    pub status_mask: i32,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DvrDomainRequest {
    Runtime(DvrRuntimeRequest),
    Unsupported { reason: &'static str, raw_fields: Vec<DomainRequestField> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvStreamKind { Video, Audio, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvStreamDomainRequest {
    pub kind: AvStreamKind,
    pub stream_type_hint: i32,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDelayKind { TimeDelayMs, DataSizeDelayBytes, InvalidOrUnknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterDelayDomainRequest {
    pub kind: FilterDelayKind,
    pub value: i32,
    pub raw_fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericDomainRequest {
    pub source_type: &'static str,
    pub fields: Vec<DomainRequestField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AidlDomainRequest {
    DemuxOpenFilter(DemuxOpenFilterRequest),
    DemuxOpenDvr(DemuxOpenDvrRequest),
    DemuxFilter(DemuxFilterDomainRequest),
    Dvr(DvrDomainRequest),
    AvStream(AvStreamDomainRequest),
    FilterDelay(FilterDelayDomainRequest),
    Generic(GenericDomainRequest),
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeExecutableRequest {
    OpenFilter(DemuxOpenFilterRequest),
    OpenDvr(DemuxOpenDvrRequest),
    ConfigureTsNoinitFilter(TsNoinitFilterRequest),
    ConfigureTsSectionFilter(TsSectionFilterRequest),
    ConfigureTsAvFilter(TsAvFilterRequest),
    ConfigureTsPesDataFilter(TsPesDataFilterRequest),
    ConfigureTsRecordFilter(TsRecordFilterRequest),
    ConfigureDvr(DvrRuntimeRequest),
    ConfigureAvStream(AvStreamDomainRequest),
    ConfigureFilterDelay(FilterDelayDomainRequest),
    UnsupportedProfile { reason: &'static str, fields: Vec<DomainRequestField> },
    Generic { source_type: &'static str, fields: Vec<DomainRequestField> },
}

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
                if request.filter_type == DemuxFilterTypeDomain::Unknown {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown DemuxFilterType"));
                }
                if request.buffer_size <= 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "filter buffer size must be positive"));
                }
                Ok(())
            }
            Self::OpenDvr(request) => {
                if request.direction == DvrDirection::Unknown {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown DvrType"));
                }
                if request.buffer_size <= 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "DVR buffer size must be positive"));
                }
                Ok(())
            }
            Self::ConfigureTsSectionFilter(request) => request.condition.validate_supported_values(),
            Self::ConfigureTsRecordFilter(request) => {
                if request.ts_index_mask < 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "record.tsIndexMask must be non-negative"));
                }
                if request.sc_index_type < 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "record.scIndexType must be non-negative"));
                }
                if let Some(mask) = request.sc_index_mask.normalized_mask {
                    if mask < 0 {
                        return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "record.scIndexMask must be non-negative when represented as a scalar mask"));
                    }
                }
                Ok(())
            }
            Self::ConfigureDvr(request) => {
                if request.packet_size <= 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "DVR packet size must be positive"));
                }
                if request.low_threshold < 0 || request.high_threshold < 0 {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "DVR thresholds must be non-negative"));
                }
                Ok(())
            }
            Self::ConfigureFilterDelay(request) if request.kind == FilterDelayKind::InvalidOrUnknown => {
                Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown FilterDelayHintType"))
            }
            Self::ConfigureAvStream(request) if request.kind == AvStreamKind::Unknown => {
                Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown AvStreamType"))
            }
            _ => Ok(()),
        }
    }
}

impl SectionConditionRequest {
    pub fn validate_supported_values(&self) -> Result<(), HalError> {
        match self {
            SectionConditionRequest::SectionBits(bits) => bits.validate_lengths(),
            SectionConditionRequest::TableInfo(table) => {
                if !(0..=0xff).contains(&table.table_id) {
                    return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "section tableId must be 0..=255"));
                }
                Ok(())
            }
            SectionConditionRequest::Unknown => Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown section condition")),
        }
    }
}

impl AidlDomainRequest {
    pub fn profile_support(&self) -> DomainProfileSupport {
        match self {
            AidlDomainRequest::DemuxOpenFilter(_) | AidlDomainRequest::DemuxOpenDvr(_) => DomainProfileSupport::Supported,
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::UnsupportedTsVariant(_))) => DomainProfileSupport::UnsupportedRecordThenUnavailable,
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(_)) => DomainProfileSupport::Supported,
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::UnsupportedVariant(_)) => DomainProfileSupport::UnsupportedRecordThenUnavailable,
            _ => DomainProfileSupport::Supported,
        }
    }

    pub fn validate_supported_values(&self) -> Result<(), HalError> {
        for field in self.fields() {
            field.validate()?;
        }
        match self {
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::UnsupportedVariant(_)) => Ok(()),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::UnsupportedTsVariant(_))) => Ok(()),
            AidlDomainRequest::Dvr(DvrDomainRequest::Unsupported { .. }) => Ok(()),
            AidlDomainRequest::FilterDelay(request) if request.kind == FilterDelayKind::InvalidOrUnknown => Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown FilterDelayHintType")),
            AidlDomainRequest::AvStream(request) if request.kind == AvStreamKind::Unknown => Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "unknown AvStreamType")),
            _ => Ok(()),
        }
    }

    pub fn into_runtime_executable_request(self) -> RuntimeExecutableRequest {
        match self {
            AidlDomainRequest::DemuxOpenFilter(request) => RuntimeExecutableRequest::OpenFilter(request),
            AidlDomainRequest::DemuxOpenDvr(request) => RuntimeExecutableRequest::OpenDvr(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Noinit(request))) => RuntimeExecutableRequest::ConfigureTsNoinitFilter(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Section(request))) => RuntimeExecutableRequest::ConfigureTsSectionFilter(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Av(request))) => RuntimeExecutableRequest::ConfigureTsAvFilter(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::PesData(request))) => RuntimeExecutableRequest::ConfigureTsPesDataFilter(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::Record(request))) => RuntimeExecutableRequest::ConfigureTsRecordFilter(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(TsFilterRuntimeRequest::UnsupportedTsVariant(request)))
            | AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::UnsupportedVariant(request)) => RuntimeExecutableRequest::UnsupportedProfile { reason: request.reason, fields: request.raw_fields },
            AidlDomainRequest::Dvr(DvrDomainRequest::Runtime(request)) => RuntimeExecutableRequest::ConfigureDvr(request),
            AidlDomainRequest::Dvr(DvrDomainRequest::Unsupported { reason, raw_fields }) => RuntimeExecutableRequest::UnsupportedProfile { reason, fields: raw_fields },
            AidlDomainRequest::AvStream(request) => RuntimeExecutableRequest::ConfigureAvStream(request),
            AidlDomainRequest::FilterDelay(request) => RuntimeExecutableRequest::ConfigureFilterDelay(request),
            AidlDomainRequest::Generic(request) => RuntimeExecutableRequest::Generic { source_type: request.source_type, fields: request.fields },
        }
    }

    pub fn fields(&self) -> &[DomainRequestField] {
        match self {
            AidlDomainRequest::DemuxOpenFilter(request) => &request.raw_fields,
            AidlDomainRequest::DemuxOpenDvr(request) => &request.raw_fields,
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::Ts(request)) => ts_filter_raw_fields(request),
            AidlDomainRequest::DemuxFilter(DemuxFilterDomainRequest::UnsupportedVariant(request)) => &request.raw_fields,
            AidlDomainRequest::Dvr(DvrDomainRequest::Runtime(request)) => &request.raw_fields,
            AidlDomainRequest::Dvr(DvrDomainRequest::Unsupported { raw_fields, .. }) => raw_fields,
            AidlDomainRequest::AvStream(request) => &request.raw_fields,
            AidlDomainRequest::FilterDelay(request) => &request.raw_fields,
            AidlDomainRequest::Generic(request) => &request.fields,
        }
    }
}

fn ts_filter_raw_fields(request: &TsFilterRuntimeRequest) -> &[DomainRequestField] {
    match request {
        TsFilterRuntimeRequest::Noinit(request) => &request.raw_fields,
        TsFilterRuntimeRequest::Section(request) => &request.raw_fields,
        TsFilterRuntimeRequest::Av(request) => &request.raw_fields,
        TsFilterRuntimeRequest::PesData(request) => &request.raw_fields,
        TsFilterRuntimeRequest::Record(request) => &request.raw_fields,
        TsFilterRuntimeRequest::UnsupportedTsVariant(request) => &request.raw_fields,
    }
}
