use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan};
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DvrCommand {
    GetQueueDesc,
    Configure(AidlDomainRequest),
    AttachFilter(AidlDomainRequest),
    DetachFilter(AidlDomainRequest),
    Start,
    Stop,
    Flush,
    Close,
    SetStatusCheckIntervalHint(i64),
}

impl DvrCommand {
    pub fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::GetQueueDesc => {
                CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrGetQueueDesc)
            }
            Self::Configure(_) => CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrConfigure),
            Self::AttachFilter(_) => {
                CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrAttachFilter)
            }
            Self::DetachFilter(_) => {
                CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrDetachFilter)
            }
            Self::Start => CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrStart),
            Self::Stop => CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrStop),
            Self::Flush => CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrFlush),
            Self::Close => CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrClose),
            Self::SetStatusCheckIntervalHint(_) => {
                CommandPlan::for_api(AidlObjectKind::Dvr, AidlApi::DvrSetStatusCheckIntervalHint)
            }
        }
    }
}
