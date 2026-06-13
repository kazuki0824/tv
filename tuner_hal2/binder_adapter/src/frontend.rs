use maleicacid_tuner_hal2_common::FrontendTuneRequest;
use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendCommand {
    Tune(FrontendTuneRequest),
    StopTune,
    Scan(FrontendTuneRequest),
    StopScan,
    Close,
    SetCallback(AidlDomainRequest),
}

impl FrontendCommand {
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::Tune(_) => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendTune, transaction: RuntimeTransactionName::FrontendTuneTxnApply },
            Self::StopTune => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendStopTune, transaction: RuntimeTransactionName::FrontendStopTuneTxn },
            Self::Scan(_) => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendScan, transaction: RuntimeTransactionName::FrontendScanTxn },
            Self::StopScan => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendStopScan, transaction: RuntimeTransactionName::FrontendStopScanTxn },
            Self::Close => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendClose, transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn },
            Self::SetCallback(_) => CommandPlan { object: AidlObjectKind::Frontend, api: AidlApi::FrontendSetCallback, transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn },
        }
    }
}
