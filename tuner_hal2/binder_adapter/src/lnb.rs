use maleicacid_tuner_hal2_common::HalError;
use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LnbCommand {
    SetCallback(AidlDomainRequest),
    SetVoltage(AidlDomainRequest),
    SetTone(AidlDomainRequest),
    SetSatellitePosition(AidlDomainRequest),
    SendDiseqc(Vec<u8>),
    Close,
}

impl LnbCommand {
    pub fn plan(&self) -> Result<CommandPlan, HalError> {
        match self {
            Self::SetCallback(_) => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSetCallback),
            Self::SetVoltage(_) => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSetVoltage),
            Self::SetTone(_) => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSetTone),
            Self::SetSatellitePosition(_) => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSetSatellitePosition),
            Self::SendDiseqc(_) => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbSendDiseqc),
            Self::Close => CommandPlan::for_api(AidlObjectKind::Lnb, AidlApi::LnbClose),
        }
    }
}
