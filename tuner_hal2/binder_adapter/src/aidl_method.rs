use maleicacid_tuner_hal2_common::FrontendTuneRequest;

use crate::demux::DemuxCommand;
use crate::descrambler::DescramblerCommand;
use crate::dvr::DvrCommand;
use crate::filter::FilterCommand;
use crate::frontend::FrontendCommand;
use crate::lnb::LnbCommand;
use crate::{domain_request_from_snapshot, CommandPlan, DomainCommand};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AidlInputField {
    pub name: &'static str,
    pub value: String,
}

impl AidlInputField {
    pub fn new(name: &'static str, value: impl Into<String>) -> Self {
        Self { name, value: value.into() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AidlInputSnapshot {
    pub source_type: &'static str,
    pub summary: String,
    pub fields: Vec<AidlInputField>,
}

impl AidlInputSnapshot {
    pub fn new(source_type: &'static str, summary: impl Into<String>) -> Self {
        let summary = summary.into();
        let fields = if summary.is_empty() {
            Vec::new()
        } else {
            vec![AidlInputField::new("summary", summary.clone())]
        };
        Self { source_type, summary, fields }
    }

    pub fn from_fields(source_type: &'static str, fields: Vec<AidlInputField>) -> Self {
        let summary = fields
            .iter()
            .map(|field| format!("{}={}", field.name, field.value))
            .collect::<Vec<_>>()
            .join(";");
        Self { source_type, summary, fields }
    }

    pub fn single_field(source_type: &'static str, name: &'static str, value: impl Into<String>) -> Self {
        Self::from_fields(source_type, vec![AidlInputField::new(name, value)])
    }

    pub fn empty(source_type: &'static str) -> Self {
        Self { source_type, summary: String::new(), fields: Vec::new() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AidlMethodCall {
    UnsupportedPublicApi { object: crate::AidlObjectKind, api: crate::AidlApi, input: Option<AidlInputSnapshot> },
    FrontendTune(FrontendTuneRequest),
    FrontendStopTune,
    FrontendScan(FrontendTuneRequest),
    FrontendStopScan,
    FrontendClose,
    FrontendSetCallback(AidlInputSnapshot),
    DemuxSetFrontendDataSource(AidlInputSnapshot),
    DemuxOpenFilter(AidlInputSnapshot),
    DemuxOpenDvr(AidlInputSnapshot),
    DemuxClose,
    FilterConfigure(AidlInputSnapshot),
    FilterConfigureAvStreamType(AidlInputSnapshot),
    FilterGetQueueDesc,
    FilterGetId,
    FilterGetId64Bit,
    FilterGetAvSharedHandle,
    FilterReleaseAvHandle(AidlInputSnapshot),
    FilterStart,
    FilterStop,
    FilterFlush,
    FilterClose,
    FilterSetDataSource(AidlInputSnapshot),
    FilterSetDelayHint(AidlInputSnapshot),
    DvrGetQueueDesc,
    DvrConfigure(AidlInputSnapshot),
    DvrAttachFilter(AidlInputSnapshot),
    DvrDetachFilter(AidlInputSnapshot),
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
    LnbSetCallback(AidlInputSnapshot),
    LnbSetVoltage(AidlInputSnapshot),
    LnbSetTone(AidlInputSnapshot),
    LnbSetSatellitePosition(AidlInputSnapshot),
    LnbSendDiseqc(Vec<u8>),
    LnbClose,
}

impl AidlMethodCall {

    pub const fn api(&self) -> crate::AidlApi {
        match self {
            Self::UnsupportedPublicApi { api, .. } => *api,
            Self::FrontendTune(_) => crate::AidlApi::FrontendTune,
            Self::FrontendStopTune => crate::AidlApi::FrontendStopTune,
            Self::FrontendScan(_) => crate::AidlApi::FrontendScan,
            Self::FrontendStopScan => crate::AidlApi::FrontendStopScan,
            Self::FrontendClose => crate::AidlApi::FrontendClose,
            Self::FrontendSetCallback(_) => crate::AidlApi::FrontendSetCallback,
            Self::DemuxSetFrontendDataSource(_) => crate::AidlApi::DemuxSetFrontendDataSource,
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
            Self::LnbSetCallback(_) => crate::AidlApi::LnbSetCallback,
            Self::LnbSetVoltage(_) => crate::AidlApi::LnbSetVoltage,
            Self::LnbSetTone(_) => crate::AidlApi::LnbSetTone,
            Self::LnbSetSatellitePosition(_) => crate::AidlApi::LnbSetSatellitePosition,
            Self::LnbSendDiseqc(_) => crate::AidlApi::LnbSendDiseqc,
            Self::LnbClose => crate::AidlApi::LnbClose,
        }
    }

    pub fn into_domain_command(self) -> DomainCommand {
        match self {
            Self::UnsupportedPublicApi { object, api, input } => DomainCommand::UnsupportedPublicApi { object, api, request: input.map(domain_request_from_snapshot) },
            Self::FrontendTune(request) => DomainCommand::Frontend(FrontendCommand::Tune(request)),
            Self::FrontendStopTune => DomainCommand::Frontend(FrontendCommand::StopTune),
            Self::FrontendScan(request) => DomainCommand::Frontend(FrontendCommand::Scan(request)),
            Self::FrontendStopScan => DomainCommand::Frontend(FrontendCommand::StopScan),
            Self::FrontendClose => DomainCommand::Frontend(FrontendCommand::Close),
            Self::FrontendSetCallback(snapshot) => DomainCommand::Frontend(FrontendCommand::SetCallback(domain_request_from_snapshot(snapshot))),
            Self::DemuxSetFrontendDataSource(snapshot) => DomainCommand::Demux(DemuxCommand::SetFrontendDataSource(domain_request_from_snapshot(snapshot))),
            Self::DemuxOpenFilter(snapshot) => DomainCommand::Demux(DemuxCommand::OpenFilter(domain_request_from_snapshot(snapshot))),
            Self::DemuxOpenDvr(snapshot) => DomainCommand::Demux(DemuxCommand::OpenDvr(domain_request_from_snapshot(snapshot))),
            Self::DemuxClose => DomainCommand::Demux(DemuxCommand::Close),
            Self::FilterConfigure(snapshot) => DomainCommand::Filter(FilterCommand::Configure(domain_request_from_snapshot(snapshot))),
            Self::FilterConfigureAvStreamType(snapshot) => DomainCommand::Filter(FilterCommand::ConfigureAvStreamType(domain_request_from_snapshot(snapshot))),
            Self::FilterGetQueueDesc => DomainCommand::Filter(FilterCommand::GetQueueDesc),
            Self::FilterGetId => DomainCommand::Filter(FilterCommand::GetId),
            Self::FilterGetId64Bit => DomainCommand::Filter(FilterCommand::GetId64Bit),
            Self::FilterGetAvSharedHandle => DomainCommand::Filter(FilterCommand::GetAvSharedHandle),
            Self::FilterReleaseAvHandle(snapshot) => DomainCommand::Filter(FilterCommand::ReleaseAvHandle(domain_request_from_snapshot(snapshot))),
            Self::FilterStart => DomainCommand::Filter(FilterCommand::Start),
            Self::FilterStop => DomainCommand::Filter(FilterCommand::Stop),
            Self::FilterFlush => DomainCommand::Filter(FilterCommand::Flush),
            Self::FilterClose => DomainCommand::Filter(FilterCommand::Close),
            Self::FilterSetDataSource(snapshot) => DomainCommand::Filter(FilterCommand::SetDataSource(domain_request_from_snapshot(snapshot))),
            Self::FilterSetDelayHint(snapshot) => DomainCommand::Filter(FilterCommand::SetDelayHint(domain_request_from_snapshot(snapshot))),
            Self::DvrGetQueueDesc => DomainCommand::Dvr(DvrCommand::GetQueueDesc),
            Self::DvrConfigure(snapshot) => DomainCommand::Dvr(DvrCommand::Configure(domain_request_from_snapshot(snapshot))),
            Self::DvrAttachFilter(snapshot) => DomainCommand::Dvr(DvrCommand::AttachFilter(domain_request_from_snapshot(snapshot))),
            Self::DvrDetachFilter(snapshot) => DomainCommand::Dvr(DvrCommand::DetachFilter(domain_request_from_snapshot(snapshot))),
            Self::DvrStart => DomainCommand::Dvr(DvrCommand::Start),
            Self::DvrStop => DomainCommand::Dvr(DvrCommand::Stop),
            Self::DvrFlush => DomainCommand::Dvr(DvrCommand::Flush),
            Self::DvrClose => DomainCommand::Dvr(DvrCommand::Close),
            Self::DvrSetStatusCheckIntervalHint(ms) => DomainCommand::Dvr(DvrCommand::SetStatusCheckIntervalHint(ms)),
            Self::DescramblerSetDemuxSource(demux_id) => DomainCommand::Descrambler(DescramblerCommand::SetDemuxSource(demux_id)),
            Self::DescramblerSetKeyToken(token) => DomainCommand::Descrambler(DescramblerCommand::SetKeyToken(token)),
            Self::DescramblerAddPid(pid) => DomainCommand::Descrambler(DescramblerCommand::AddPid(pid)),
            Self::DescramblerRemovePid(pid) => DomainCommand::Descrambler(DescramblerCommand::RemovePid(pid)),
            Self::DescramblerClose => DomainCommand::Descrambler(DescramblerCommand::Close),
            Self::LnbSetCallback(snapshot) => DomainCommand::Lnb(LnbCommand::SetCallback(domain_request_from_snapshot(snapshot))),
            Self::LnbSetVoltage(snapshot) => DomainCommand::Lnb(LnbCommand::SetVoltage(domain_request_from_snapshot(snapshot))),
            Self::LnbSetTone(snapshot) => DomainCommand::Lnb(LnbCommand::SetTone(domain_request_from_snapshot(snapshot))),
            Self::LnbSetSatellitePosition(snapshot) => DomainCommand::Lnb(LnbCommand::SetSatellitePosition(domain_request_from_snapshot(snapshot))),
            Self::LnbSendDiseqc(message) => DomainCommand::Lnb(LnbCommand::SendDiseqc(message)),
            Self::LnbClose => DomainCommand::Lnb(LnbCommand::Close),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AidlMethodPlan {
    pub api: crate::AidlApi,
    pub method: AidlMethodCall,
    pub command: DomainCommand,
    pub command_plan: CommandPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AidlMethodAdapter {
    covered_method_count: usize,
}

impl Default for AidlMethodAdapter {
    fn default() -> Self { Self::new() }
}

impl AidlMethodAdapter {
    pub fn new() -> Self { Self { covered_method_count: crate::AIDL_TRANSACTION_TABLE.len() } }
    pub const fn covered_method_count(&self) -> usize { self.covered_method_count }

    pub fn plan(method: AidlMethodCall) -> AidlMethodPlan {
        let api = method.api();
        let command = method.clone().into_domain_command();
        let command_plan = command.plan();
        AidlMethodPlan { api, method, command, command_plan }
    }

    pub fn frontend_tune(request: FrontendTuneRequest) -> AidlMethodPlan { Self::plan(AidlMethodCall::FrontendTune(request)) }
    pub fn frontend_stop_tune() -> AidlMethodPlan { Self::plan(AidlMethodCall::FrontendStopTune) }
    pub fn frontend_scan(request: FrontendTuneRequest) -> AidlMethodPlan { Self::plan(AidlMethodCall::FrontendScan(request)) }
    pub fn frontend_stop_scan() -> AidlMethodPlan { Self::plan(AidlMethodCall::FrontendStopScan) }
    pub fn frontend_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::FrontendClose) }

    pub fn demux_open_filter() -> AidlMethodPlan { Self::plan(AidlMethodCall::DemuxOpenFilter(AidlInputSnapshot::empty("DemuxOpenFilter"))) }
    pub fn demux_open_dvr() -> AidlMethodPlan { Self::plan(AidlMethodCall::DemuxOpenDvr(AidlInputSnapshot::empty("DemuxOpenDvr"))) }
    pub fn demux_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::DemuxClose) }

    pub fn filter_configure() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterConfigure(AidlInputSnapshot::empty("DemuxFilterSettings"))) }
    pub fn filter_get_queue_desc() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterGetQueueDesc) }
    pub fn filter_get_id() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterGetId) }
    pub fn filter_get_id64_bit() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterGetId64Bit) }
    pub fn filter_get_av_shared_handle() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterGetAvSharedHandle) }
    pub fn filter_release_av_handle() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterReleaseAvHandle(AidlInputSnapshot::empty("releaseAvHandle"))) }
    pub fn filter_start() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterStart) }
    pub fn filter_stop() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterStop) }
    pub fn filter_flush() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterFlush) }
    pub fn filter_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::FilterClose) }

    pub fn dvr_get_queue_desc() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrGetQueueDesc) }
    pub fn dvr_configure() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrConfigure(AidlInputSnapshot::empty("DvrSettings"))) }
    pub fn dvr_start() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrStart) }
    pub fn dvr_stop() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrStop) }
    pub fn dvr_flush() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrFlush) }
    pub fn dvr_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::DvrClose) }

    pub fn descrambler_set_demux_source() -> AidlMethodPlan { Self::plan(AidlMethodCall::DescramblerSetDemuxSource(0)) }
    pub fn descrambler_set_key_token(token: Vec<u8>) -> AidlMethodPlan { Self::plan(AidlMethodCall::DescramblerSetKeyToken(token)) }
    pub fn descrambler_add_pid(pid: u16) -> AidlMethodPlan { Self::plan(AidlMethodCall::DescramblerAddPid(pid)) }
    pub fn descrambler_remove_pid(pid: u16) -> AidlMethodPlan { Self::plan(AidlMethodCall::DescramblerRemovePid(pid)) }
    pub fn descrambler_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::DescramblerClose) }

    pub fn lnb_set_voltage() -> AidlMethodPlan { Self::plan(AidlMethodCall::LnbSetVoltage(AidlInputSnapshot::empty("LnbVoltage"))) }
    pub fn lnb_set_tone() -> AidlMethodPlan { Self::plan(AidlMethodCall::LnbSetTone(AidlInputSnapshot::empty("LnbTone"))) }
    pub fn lnb_set_satellite_position() -> AidlMethodPlan { Self::plan(AidlMethodCall::LnbSetSatellitePosition(AidlInputSnapshot::empty("LnbPosition"))) }
    pub fn lnb_send_diseqc() -> AidlMethodPlan { Self::plan(AidlMethodCall::LnbSendDiseqc(Vec::new())) }
    pub fn lnb_close() -> AidlMethodPlan { Self::plan(AidlMethodCall::LnbClose) }

    pub fn unsupported_public_api(object: crate::AidlObjectKind, api: crate::AidlApi) -> AidlMethodPlan {
        Self::plan(AidlMethodCall::UnsupportedPublicApi { object, api, input: None })
    }

    pub fn unsupported_public_api_with_input(object: crate::AidlObjectKind, api: crate::AidlApi, input: AidlInputSnapshot) -> AidlMethodPlan {
        Self::plan(AidlMethodCall::UnsupportedPublicApi { object, api, input: Some(input) })
    }
}

pub fn all_aidl_method_kinds_for_coverage(sample_request: FrontendTuneRequest) -> Vec<AidlMethodCall> {
    let empty = |name| AidlInputSnapshot::empty(name);
    vec![
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Tuner, api: crate::AidlApi::TunerGetFrontendIds, input: None },
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Tuner, api: crate::AidlApi::TunerGetDemuxCaps, input: None },
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Tuner, api: crate::AidlApi::TunerGetLnbIds, input: None },
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Tuner, api: crate::AidlApi::TunerGetDemuxIds, input: None },
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Frontend, api: crate::AidlApi::FrontendGetStatus, input: Some(empty("FrontendStatusTypes")) },
        AidlMethodCall::UnsupportedPublicApi { object: crate::AidlObjectKind::Demux, api: crate::AidlApi::DemuxOpenTimeFilter, input: None },
        AidlMethodCall::FrontendTune(sample_request.clone()),
        AidlMethodCall::FrontendStopTune,
        AidlMethodCall::FrontendScan(sample_request),
        AidlMethodCall::FrontendStopScan,
        AidlMethodCall::FrontendClose,
        AidlMethodCall::FrontendSetCallback(empty("IFrontendCallback")),
        AidlMethodCall::DemuxSetFrontendDataSource(empty("frontendId")),
        AidlMethodCall::DemuxOpenFilter(empty("DemuxOpenFilter")),
        AidlMethodCall::DemuxOpenDvr(empty("DemuxOpenDvr")),
        AidlMethodCall::DemuxClose,
        AidlMethodCall::FilterConfigure(empty("DemuxFilterSettings")),
        AidlMethodCall::FilterConfigureAvStreamType(empty("AvStreamType")),
        AidlMethodCall::FilterGetQueueDesc,
        AidlMethodCall::FilterGetId,
        AidlMethodCall::FilterGetId64Bit,
        AidlMethodCall::FilterGetAvSharedHandle,
        AidlMethodCall::FilterReleaseAvHandle(empty("releaseAvHandle")),
        AidlMethodCall::FilterStart,
        AidlMethodCall::FilterStop,
        AidlMethodCall::FilterFlush,
        AidlMethodCall::FilterClose,
        AidlMethodCall::FilterSetDataSource(empty("IFilter")),
        AidlMethodCall::FilterSetDelayHint(empty("FilterDelayHint")),
        AidlMethodCall::DvrGetQueueDesc,
        AidlMethodCall::DvrConfigure(empty("DvrSettings")),
        AidlMethodCall::DvrAttachFilter(empty("IFilter")),
        AidlMethodCall::DvrDetachFilter(empty("IFilter")),
        AidlMethodCall::DvrStart,
        AidlMethodCall::DvrStop,
        AidlMethodCall::DvrFlush,
        AidlMethodCall::DvrClose,
        AidlMethodCall::DvrSetStatusCheckIntervalHint(1000),
        AidlMethodCall::DescramblerSetDemuxSource(1),
        AidlMethodCall::DescramblerSetKeyToken(vec![1, 2, 3, 4]),
        AidlMethodCall::DescramblerAddPid(0x100),
        AidlMethodCall::DescramblerRemovePid(0x100),
        AidlMethodCall::DescramblerClose,
        AidlMethodCall::LnbSetCallback(empty("ILnbCallback")),
        AidlMethodCall::LnbSetVoltage(empty("LnbVoltage")),
        AidlMethodCall::LnbSetTone(empty("LnbTone")),
        AidlMethodCall::LnbSetSatellitePosition(empty("LnbPosition")),
        AidlMethodCall::LnbSendDiseqc(vec![0xe0, 0x10]),
        AidlMethodCall::LnbClose,
    ]
}
