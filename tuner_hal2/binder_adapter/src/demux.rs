use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan};
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DemuxCommand {
    SetFrontendDataSource(AidlDomainRequest),
    OpenFilter(AidlDomainRequest),
    OpenDvr(AidlDomainRequest),
    Close,
}

impl DemuxCommand {
    pub fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::SetFrontendDataSource(_) => {
                CommandPlan::for_api(AidlObjectKind::Demux, AidlApi::DemuxSetFrontendDataSource)
            }
            Self::OpenFilter(_) => {
                CommandPlan::for_api(AidlObjectKind::Demux, AidlApi::DemuxOpenFilter)
            }
            Self::OpenDvr(_) => CommandPlan::for_api(AidlObjectKind::Demux, AidlApi::DemuxOpenDvr),
            Self::Close => CommandPlan::for_api(AidlObjectKind::Demux, AidlApi::DemuxClose),
        }
    }
}
