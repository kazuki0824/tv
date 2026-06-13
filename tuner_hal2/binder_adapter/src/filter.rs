use crate::AidlDomainRequest;
use crate::{AidlApi, AidlObjectKind, CommandPlan, RuntimeTransactionName};

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
    pub fn plan(&self) -> CommandPlan {
        match self {
            Self::Configure(_) => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterConfigure, transaction: RuntimeTransactionName::FilterConfigureTxn },
            Self::ConfigureAvStreamType(_) => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterConfigureAvStreamType, transaction: RuntimeTransactionName::FilterConfigureTxn },
            Self::GetQueueDesc => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetQueueDesc, transaction: RuntimeTransactionName::FilterGetQueueDescTxn },
            Self::GetId => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetId, transaction: RuntimeTransactionName::FilterGetIdTxn },
            Self::GetId64Bit => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetId64Bit, transaction: RuntimeTransactionName::FilterGetId64BitTxn },
            Self::GetAvSharedHandle => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterGetAvSharedHandle, transaction: RuntimeTransactionName::FilterGetAvSharedHandleTxn },
            Self::ReleaseAvHandle(_) => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterReleaseAvHandle, transaction: RuntimeTransactionName::FilterReleaseAvHandleTxn },
            Self::Start => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterStart, transaction: RuntimeTransactionName::FilterStartTxn },
            Self::Stop => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterStop, transaction: RuntimeTransactionName::FilterStopTxn },
            Self::Flush => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterFlush, transaction: RuntimeTransactionName::FilterFlushTxn },
            Self::Close => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterClose, transaction: RuntimeTransactionName::FilterCloseLifecycleTxn },
            Self::SetDataSource(_) => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterSetDataSource, transaction: RuntimeTransactionName::FilterSetDataSourceTxn },
            Self::SetDelayHint(_) => CommandPlan { object: AidlObjectKind::Filter, api: AidlApi::FilterSetDelayHint, transaction: RuntimeTransactionName::FilterConfigureTxn },
        }
    }
}
