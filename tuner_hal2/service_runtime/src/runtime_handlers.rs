use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, RuntimeTransactionName,
};

use crate::command_dispatch::RuntimeCommandDispatchPlan;
use crate::object_table::RuntimeObjectTable;
use crate::runtime_result::{
    RuntimeHandlerCoverage, RuntimeHandlerError, RuntimeHandlerResult, RuntimeHandlerSuccess,
};
use crate::transaction_registry::{
    every_aidl_transaction_has_runtime_spec, transaction_spec_count, transaction_spec_for,
};

pub fn runtime_handler_coverage_for(transaction: RuntimeTransactionName) -> RuntimeHandlerCoverage {
    transaction_spec_for(transaction)
        .map(|spec| spec.handler_coverage)
        .unwrap_or(RuntimeHandlerCoverage::NotConnected)
}

pub fn all_runtime_transactions_are_classified() -> bool {
    every_aidl_transaction_has_runtime_spec()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeDispatchHandler {
    classified_transaction_count: usize,
}

impl Default for RuntimeDispatchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeDispatchHandler {
    pub fn new() -> Self {
        Self {
            classified_transaction_count: transaction_spec_count(),
        }
    }

    pub const fn classified_transaction_count(&self) -> usize {
        self.classified_transaction_count
    }

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
                crate::object_table::RuntimeObjectTableError::MissingObject { .. } => {
                    Err(RuntimeHandlerError::MissingObject {
                        transaction,
                        source: err,
                    })
                }
                crate::object_table::RuntimeObjectTableError::GenerationMismatch { .. } => {
                    Err(RuntimeHandlerError::GenerationMismatch {
                        transaction,
                        source: err,
                    })
                }
                crate::object_table::RuntimeObjectTableError::ObjectKindMismatch { .. }
                | crate::object_table::RuntimeObjectTableError::InvalidOwner { .. }
                | crate::object_table::RuntimeObjectTableError::MissingOwner { .. }
                | crate::object_table::RuntimeObjectTableError::OwnerGenerationMismatch {
                    ..
                }
                | crate::object_table::RuntimeObjectTableError::OwnerKindMismatch { .. }
                | crate::object_table::RuntimeObjectTableError::OwnerNotLive { .. }
                | crate::object_table::RuntimeObjectTableError::InvalidLifecycle { .. }
                | crate::object_table::RuntimeObjectTableError::DuplicateObjectId { .. }
                | crate::object_table::RuntimeObjectTableError::DuplicateRuntimeBinding {
                    ..
                }
                | crate::object_table::RuntimeObjectTableError::UnsupportedObjectKind { .. }
                | crate::object_table::RuntimeObjectTableError::GenerationOverflow => {
                    Err(RuntimeHandlerError::InvalidOwner {
                        transaction,
                        source: err,
                    })
                }
            };
        }

        if let Some(request) = dispatch_plan.executable_request.as_ref() {
            if request.profile_support() == maleicacid_tuner_hal2_domain_request::DomainProfileSupport::UnsupportedRecordThenUnavailable {
                return Err(RuntimeHandlerError::UnsupportedProfile { transaction });
            }
            if let Err(error) = request.validate_supported_values() {
                return Err(RuntimeHandlerError::InputValidation {
                    transaction,
                    source: error,
                });
            }
        }

        match runtime_handler_coverage_for(transaction) {
            RuntimeHandlerCoverage::Connected => Ok(RuntimeHandlerResult {
                transaction,
                target,
                success: RuntimeHandlerSuccess::Planned,
            }),
            RuntimeHandlerCoverage::UnsupportedByDesign => Ok(RuntimeHandlerResult {
                transaction,
                target,
                success: RuntimeHandlerSuccess::UnsupportedByDesign,
            }),
            RuntimeHandlerCoverage::NotConnected => Err(RuntimeHandlerError::NotConnected {
                transaction,
                target,
            }),
        }
    }
}
