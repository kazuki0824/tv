use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DemuxCommand {
    SetFrontendDataSource(AidlDomainRequest),
    OpenFilter(AidlDomainRequest),
    OpenDvr(AidlDomainRequest),
    Close,
}

impl DemuxCommand {
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::SetFrontendDataSource(_) => CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxSetFrontendDataSource, transaction: RuntimeTransactionName::DemuxSetFrontendDataSourceTxn },
            Self::OpenFilter(_) => CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxOpenFilter, transaction: RuntimeTransactionName::DemuxOpenFilterTxn },
            Self::OpenDvr(_) => CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxOpenDvr, transaction: RuntimeTransactionName::DemuxOpenDvrTxn },
            Self::Close => CommandPlan { object: AidlObjectKind::Demux, api: AidlApi::DemuxClose, transaction: RuntimeTransactionName::DemuxCloseLifecycleTxn },
        }
    }
}
