use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};
use maleicacid_tuner_hal2_common::FrontendTuneRequest;

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
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::Tune(_) => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendTune,
                transaction: RuntimeTransactionName::FrontendTuneTxnApply,
            },
            Self::SetLnb(_) => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendSetLnb,
                transaction: RuntimeTransactionName::LnbApplyTxn,
            },
            Self::StopTune => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendStopTune,
                transaction: RuntimeTransactionName::FrontendStopTuneTxn,
            },
            Self::Scan(_) => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendScan,
                transaction: RuntimeTransactionName::FrontendScanTxn,
            },
            Self::StopScan => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendStopScan,
                transaction: RuntimeTransactionName::FrontendStopScanTxn,
            },
            Self::Close => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendClose,
                transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn,
            },
            Self::SetCallback(_) => CommandPlan {
                object: AidlObjectKind::Frontend,
                api: AidlApi::FrontendSetCallback,
                transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn,
            },
        }
    }
}
