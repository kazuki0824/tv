use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

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
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::SetCallback(_) => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetCallback, transaction: RuntimeTransactionName::LnbApplyTxn },
            Self::SetVoltage(_) => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetVoltage, transaction: RuntimeTransactionName::LnbApplyTxn },
            Self::SetTone(_) => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetTone, transaction: RuntimeTransactionName::LnbApplyTxn },
            Self::SetSatellitePosition(_) => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSetSatellitePosition, transaction: RuntimeTransactionName::LnbApplyTxn },
            Self::SendDiseqc(_) => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbSendDiseqc, transaction: RuntimeTransactionName::LnbApplyTxn },
            Self::Close => CommandPlan { object: AidlObjectKind::Lnb, api: AidlApi::LnbClose, transaction: RuntimeTransactionName::LnbLifecycleTxnClose },
        }
    }
}
