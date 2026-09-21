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

pub fn missing_adapter_transactions() -> Vec<RuntimeTransactionName> {
    AIDL_TRANSACTION_TABLE
        .iter()
        .filter_map(|plan| {
            let transaction = plan.transaction();
            dispatch_target_for(transaction).is_none().then_some(transaction)
        })
        .collect()
}

pub fn adapter_transactions_are_covered() -> bool {
    missing_adapter_transactions().is_empty()
}
