use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan};
use maleicacid_tuner_hal2_common::FrontendTuneRequest;
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendCommand {
    Tune(FrontendTuneRequest),
    SetLnb(i32),
    StopTune,
    Scan(FrontendTuneRequest),
    StopScan,
    Close,
    SetCallback(AidlDomainRequest),
}

impl FrontendCommand {
    pub(super) fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::Tune(_) => CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendTune),
            Self::SetLnb(_) => {
                CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendSetLnb)
            }
            Self::StopTune => {
                CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendStopTune)
            }
            Self::Scan(_) => CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendScan),
            Self::StopScan => {
                CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendStopScan)
            }
            Self::Close => CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendClose),
            Self::SetCallback(_) => {
                CommandPlan::for_api(AidlObjectKind::Frontend, AidlApi::FrontendSetCallback)
            }
        }
    }
}
