use maleicacid_tuner_hal2_common::HalError;
use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilterCommand {
    Configure(AidlDomainRequest),
    ConfigureAvStreamType(AidlDomainRequest),
    GetQueueDesc,
    GetId,
    GetId64Bit,
    GetAvSharedHandle,
    ReleaseAvHandle(AidlDomainRequest),
    Start,
    Stop,
    Flush,
    Close,
    SetDataSource(AidlDomainRequest),
    SetDelayHint(AidlDomainRequest),
}

impl FilterCommand {
    pub fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::Configure(_) => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterConfigure),
            Self::ConfigureAvStreamType(_) => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterConfigureAvStreamType),
            Self::GetQueueDesc => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterGetQueueDesc),
            Self::GetId => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterGetId),
            Self::GetId64Bit => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterGetId64Bit),
            Self::GetAvSharedHandle => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterGetAvSharedHandle),
            Self::ReleaseAvHandle(_) => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterReleaseAvHandle),
            Self::Start => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterStart),
            Self::Stop => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterStop),
            Self::Flush => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterFlush),
            Self::Close => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterClose),
            Self::SetDataSource(_) => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterSetDataSource),
            Self::SetDelayHint(_) => CommandPlan::for_api(AidlObjectKind::Filter, AidlApi::FilterSetDelayHint),
        }
    }
}
