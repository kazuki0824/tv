use maleicacid_tuner_hal2_domain_request::{RuntimeTransactionName, AIDL_TRANSACTION_TABLE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceRuntimeDispatchTarget {
    Tuner,
    Frontend,
    Demux,
    Filter,
    Dvr,
    Descrambler,
    Lnb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeDispatchEntry {
    pub transaction: RuntimeTransactionName,
    pub target: ServiceRuntimeDispatchTarget,
}

pub const SERVICE_RUNTIME_DISPATCH_TABLE: &[RuntimeDispatchEntry] = &[
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn, target: ServiceRuntimeDispatchTarget::Tuner },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn, target: ServiceRuntimeDispatchTarget::Demux },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendTuneTxnApply, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendStopTuneTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendScanTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendStopScanTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn, target: ServiceRuntimeDispatchTarget::Frontend },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DemuxSetFrontendDataSourceTxn, target: ServiceRuntimeDispatchTarget::Demux },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DemuxOpenFilterTxn, target: ServiceRuntimeDispatchTarget::Demux },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DemuxOpenDvrTxn, target: ServiceRuntimeDispatchTarget::Demux },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DemuxCloseLifecycleTxn, target: ServiceRuntimeDispatchTarget::Demux },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterConfigureTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterGetQueueDescTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterGetIdTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterGetId64BitTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterGetAvSharedHandleTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterReleaseAvHandleTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterStartTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterStopTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterFlushTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterCloseLifecycleTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::FilterSetDataSourceTxn, target: ServiceRuntimeDispatchTarget::Filter },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrGetQueueDescTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrConfigureTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrStartTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrStopTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrFlushTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DvrCloseLifecycleTxn, target: ServiceRuntimeDispatchTarget::Dvr },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource, target: ServiceRuntimeDispatchTarget::Descrambler },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnSetKeyToken, target: ServiceRuntimeDispatchTarget::Descrambler },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnAddPid, target: ServiceRuntimeDispatchTarget::Descrambler },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnRemovePid, target: ServiceRuntimeDispatchTarget::Descrambler },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnClose, target: ServiceRuntimeDispatchTarget::Descrambler },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::LnbApplyTxn, target: ServiceRuntimeDispatchTarget::Lnb },
    RuntimeDispatchEntry { transaction: RuntimeTransactionName::LnbLifecycleTxnClose, target: ServiceRuntimeDispatchTarget::Lnb },
];

pub fn dispatch_target_for(transaction: RuntimeTransactionName) -> Option<ServiceRuntimeDispatchTarget> {
    SERVICE_RUNTIME_DISPATCH_TABLE
        .iter()
        .find(|entry| entry.transaction == transaction)
        .map(|entry| entry.target)
}

pub fn adapter_transactions_are_covered() -> bool {
    AIDL_TRANSACTION_TABLE
        .iter()
        .all(|plan| dispatch_target_for(plan.transaction).is_some())
}
