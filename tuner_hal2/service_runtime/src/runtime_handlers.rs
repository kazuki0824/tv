use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AIDL_TRANSACTION_TABLE, RuntimeTransactionName};

use crate::command_dispatch::RuntimeCommandDispatchPlan;
use crate::object_table::RuntimeObjectTable;
use crate::runtime_result::{RuntimeHandlerCoverage, RuntimeHandlerError, RuntimeHandlerResult, RuntimeHandlerSuccess};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeHandlerCoverageEntry {
    pub transaction: RuntimeTransactionName,
    pub coverage: RuntimeHandlerCoverage,
}

pub const RUNTIME_HANDLER_COVERAGE_TABLE: &[RuntimeHandlerCoverageEntry] = &[
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendTuneTxnApply, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendStopTuneTxn, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendScanTxn, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendStopScanTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendCloseLifecycleTxn, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FrontendCallbackRegistrationTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DemuxSetFrontendDataSourceTxn, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DemuxOpenFilterTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DemuxOpenDvrTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DemuxCloseLifecycleTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterConfigureTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterGetQueueDescTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterGetAvSharedHandleTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterReleaseAvHandleTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterStartTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterStopTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterFlushTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterCloseLifecycleTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::FilterSetDataSourceTxn, coverage: RuntimeHandlerCoverage::Connected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrGetQueueDescTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrConfigureTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrStartTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrStopTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrFlushTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DvrCloseLifecycleTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnSetDemuxSource, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnSetKeyToken, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnAddPid, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnRemovePid, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::DescramblerSessionTxnClose, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::LnbApplyTxn, coverage: RuntimeHandlerCoverage::NotConnected },
    RuntimeHandlerCoverageEntry { transaction: RuntimeTransactionName::LnbLifecycleTxnClose, coverage: RuntimeHandlerCoverage::NotConnected },
];

pub fn runtime_handler_coverage_for(transaction: RuntimeTransactionName) -> RuntimeHandlerCoverage {
    RUNTIME_HANDLER_COVERAGE_TABLE
        .iter()
        .find(|entry| entry.transaction == transaction)
        .map(|entry| entry.coverage)
        .unwrap_or(RuntimeHandlerCoverage::NotConnected)
}

pub fn all_runtime_transactions_are_classified() -> bool {
    AIDL_TRANSACTION_TABLE
        .iter()
        .all(|plan| RUNTIME_HANDLER_COVERAGE_TABLE.iter().any(|entry| entry.transaction == plan.transaction))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeDispatchHandler {
    classified_transaction_count: usize,
}

impl Default for RuntimeDispatchHandler {
    fn default() -> Self { Self::new() }
}

impl RuntimeDispatchHandler {
    pub fn new() -> Self {
        Self { classified_transaction_count: RUNTIME_HANDLER_COVERAGE_TABLE.len() }
    }

    pub const fn classified_transaction_count(&self) -> usize { self.classified_transaction_count }

    pub fn dispatch(
        dispatch_plan: &RuntimeCommandDispatchPlan,
        object_table: &RuntimeObjectTable,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    ) -> Result<RuntimeHandlerResult, RuntimeHandlerError> {
        let transaction = dispatch_plan.command_plan.transaction;
        let target = dispatch_plan.target;
        let object_kind = dispatch_plan.command_plan.object;
        let object_check = object_table.entry_for_kind(object_id, object_generation, object_kind);
        if let Err(err) = object_check {
            return match err {
                crate::object_table::RuntimeObjectTableError::MissingObject { .. } => Err(RuntimeHandlerError::MissingObject { transaction, source: err }),
                crate::object_table::RuntimeObjectTableError::GenerationMismatch { .. } => Err(RuntimeHandlerError::GenerationMismatch { transaction, source: err }),
                crate::object_table::RuntimeObjectTableError::ObjectKindMismatch { .. }
                | crate::object_table::RuntimeObjectTableError::InvalidOwner { .. }
                | crate::object_table::RuntimeObjectTableError::MissingOwner { .. }
                | crate::object_table::RuntimeObjectTableError::OwnerGenerationMismatch { .. }
                | crate::object_table::RuntimeObjectTableError::OwnerKindMismatch { .. }
                | crate::object_table::RuntimeObjectTableError::OwnerNotLive { .. }
                | crate::object_table::RuntimeObjectTableError::InvalidLifecycle { .. }
                | crate::object_table::RuntimeObjectTableError::DuplicateObjectId { .. }
                | crate::object_table::RuntimeObjectTableError::DuplicateRuntimeBinding { .. }
                | crate::object_table::RuntimeObjectTableError::UnsupportedObjectKind { .. }
                | crate::object_table::RuntimeObjectTableError::GenerationOverflow => {
                    Err(RuntimeHandlerError::InvalidOwner { transaction, source: err })
                }
            };
        }

        if let Some(request) = dispatch_plan.executable_request.as_ref() {
            if let Err(error) = request.validate_supported_values() {
                return Err(RuntimeHandlerError::InputValidation { transaction, source: error });
            }
            if request.profile_support() == maleicacid_tuner_hal2_domain_request::DomainProfileSupport::UnsupportedRecordThenUnavailable {
                return Err(RuntimeHandlerError::UnsupportedProfile { transaction });
            }
        }

        match runtime_handler_coverage_for(transaction) {
            RuntimeHandlerCoverage::Connected => Ok(RuntimeHandlerResult { transaction, target, success: RuntimeHandlerSuccess::Planned }),
            RuntimeHandlerCoverage::UnsupportedByDesign => Ok(RuntimeHandlerResult { transaction, target, success: RuntimeHandlerSuccess::UnsupportedByDesign }),
            RuntimeHandlerCoverage::NotConnected => Err(RuntimeHandlerError::NotConnected { transaction, target }),
        }
    }
}
