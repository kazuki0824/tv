use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

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
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::GetQueueDesc => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrGetQueueDesc,
                transaction: RuntimeTransactionName::DvrGetQueueDescTxn,
            },
            Self::Configure(_) => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrConfigure,
                transaction: RuntimeTransactionName::DvrConfigureTxn,
            },
            Self::AttachFilter(_) => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrAttachFilter,
                transaction: RuntimeTransactionName::DvrConfigureTxn,
            },
            Self::DetachFilter(_) => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrDetachFilter,
                transaction: RuntimeTransactionName::DvrConfigureTxn,
            },
            Self::Start => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrStart,
                transaction: RuntimeTransactionName::DvrStartTxn,
            },
            Self::Stop => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrStop,
                transaction: RuntimeTransactionName::DvrStopTxn,
            },
            Self::Flush => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrFlush,
                transaction: RuntimeTransactionName::DvrFlushTxn,
            },
            Self::Close => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrClose,
                transaction: RuntimeTransactionName::DvrCloseLifecycleTxn,
            },
            Self::SetStatusCheckIntervalHint(_) => CommandPlan {
                object: AidlObjectKind::Dvr,
                api: AidlApi::DvrSetStatusCheckIntervalHint,
                transaction: RuntimeTransactionName::DvrConfigureTxn,
            },
        }
    }
}
