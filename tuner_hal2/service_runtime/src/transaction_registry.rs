use maleicacid_tuner_hal2_domain_request::{RuntimeTransactionName, AIDL_TRANSACTION_TABLE};

use crate::dispatch::ServiceRuntimeDispatchTarget;
pub type RuntimeDispatchTarget = ServiceRuntimeDispatchTarget;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeTransactionCoverage {
    Connected,
    NotConnected,
    UnsupportedByDesign,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeTransactionSpec {
    pub transaction: RuntimeTransactionName,
    pub dispatch_target: RuntimeDispatchTarget,
    pub runtime_coverage: RuntimeTransactionCoverage,
}

pub const RUNTIME_TRANSACTION_SPECS: &[RuntimeTransactionSpec] = &[
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::TunerPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Tuner,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::TunerUnsupportedPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Tuner,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendTuneTxnApply,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendStopTuneTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendScanTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendStopScanTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn,
        dispatch_target: RuntimeDispatchTarget::Frontend,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxSetFrontendDataSourceTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxOpenFilterTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxOpenDvrTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DemuxCloseLifecycleTxn,
        dispatch_target: RuntimeDispatchTarget::Demux,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterConfigureTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterGetQueueDescTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterGetIdTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterGetId64BitTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterGetAvSharedHandleTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterReleaseAvHandleTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterStartTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterStopTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterFlushTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterCloseLifecycleTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::FilterSetDataSourceTxn,
        dispatch_target: RuntimeDispatchTarget::Filter,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrGetQueueDescTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrConfigureTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrStartTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrStopTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrFlushTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DvrCloseLifecycleTxn,
        dispatch_target: RuntimeDispatchTarget::Dvr,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource,
        dispatch_target: RuntimeDispatchTarget::Descrambler,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DescramblerSessionTxnSetKeyToken,
        dispatch_target: RuntimeDispatchTarget::Descrambler,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DescramblerSessionTxnAddPid,
        dispatch_target: RuntimeDispatchTarget::Descrambler,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DescramblerSessionTxnRemovePid,
        dispatch_target: RuntimeDispatchTarget::Descrambler,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::DescramblerSessionTxnClose,
        dispatch_target: RuntimeDispatchTarget::Descrambler,
        runtime_coverage: RuntimeTransactionCoverage::Connected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::LnbApplyTxn,
        dispatch_target: RuntimeDispatchTarget::Lnb,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
    RuntimeTransactionSpec {
        transaction: RuntimeTransactionName::LnbLifecycleTxnClose,
        dispatch_target: RuntimeDispatchTarget::Lnb,
        runtime_coverage: RuntimeTransactionCoverage::NotConnected,
    },
];

pub fn runtime_transaction_specs() -> &'static [RuntimeTransactionSpec] {
    RUNTIME_TRANSACTION_SPECS
}

pub fn transaction_spec_for(
    transaction: RuntimeTransactionName,
) -> Option<&'static RuntimeTransactionSpec> {
    RUNTIME_TRANSACTION_SPECS
        .iter()
        .find(|spec| spec.transaction == transaction)
}

pub fn transaction_spec_count() -> usize {
    RUNTIME_TRANSACTION_SPECS.len()
}

pub fn every_aidl_transaction_has_runtime_spec() -> bool {
    AIDL_TRANSACTION_TABLE
        .iter()
        .all(|plan| transaction_spec_for(plan.transaction()).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_domain_request::AIDL_TRANSACTION_TABLE;

    #[test]
    fn every_aidl_transaction_has_runtime_spec_entry() {
        for plan in AIDL_TRANSACTION_TABLE {
            assert!(
                transaction_spec_for(plan.transaction()).is_some(),
                "missing runtime transaction spec for {:?}",
                plan.transaction()
            );
        }
    }

    #[test]
    fn runtime_specs_do_not_duplicate_transaction_names() {
        for (index, spec) in RUNTIME_TRANSACTION_SPECS.iter().enumerate() {
            assert_eq!(
                RUNTIME_TRANSACTION_SPECS
                    .iter()
                    .filter(|candidate| candidate.transaction == spec.transaction)
                    .count(),
                1,
                "duplicated runtime transaction spec at index {} for {:?}",
                index,
                spec.transaction
            );
        }
    }
}
