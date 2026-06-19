use maleicacid_tuner_hal2_domain_request::{RuntimeTransactionName, AIDL_TRANSACTION_TABLE};

use crate::transaction_registry::transaction_spec_for;

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

pub fn dispatch_target_for(
    transaction: RuntimeTransactionName,
) -> Option<ServiceRuntimeDispatchTarget> {
    transaction_spec_for(transaction).map(|spec| spec.dispatch_target)
}

pub fn adapter_transactions_are_covered() -> bool {
    AIDL_TRANSACTION_TABLE
        .iter()
        .all(|plan| dispatch_target_for(plan.transaction()).is_some())
}
