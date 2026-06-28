use crate::{
    error_mapping::object_table_error_to_hal, RuntimeObjectEntry, RuntimeObjectLifecycle,
    TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_resource_ledger::LedgerId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidlObjectCloseability {
    BeginClose,
}

pub fn aidl_object_entry_for_kind(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<RuntimeObjectEntry, HalError> {
    runtime
        .object_table()
        .entry_for_kind(object_id, generation, expected_kind)
        .cloned()
        .map_err(object_table_error_to_hal)
}

pub fn aidl_object_live(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<(), HalError> {
    aidl_object_entry_for_kind(runtime, object_id, generation, expected_kind).map(|_| ())
}

pub fn aidl_object_closeable(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<AidlObjectCloseability, HalError> {
    let entry = runtime.object_table().entry(object_id).ok_or_else(|| {
        HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is missing",
        )
    })?;
    if entry.generation != generation {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object generation mismatch",
        ));
    }
    if entry.object_kind != expected_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch",
        ));
    }
    match entry.lifecycle {
        RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupFailed { .. } => {
            Ok(AidlObjectCloseability::BeginClose)
        }
        RuntimeObjectLifecycle::Closed
        | RuntimeObjectLifecycle::Closing { .. }
        | RuntimeObjectLifecycle::Quarantined => Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is not closeable",
        )),
    }
}

pub fn aidl_object_for_close_cleanup_runtime(
    runtime: &TunerServiceRuntime,
    expected_kind: AidlObjectKind,
    public_runtime_id: i64,
) -> Option<RuntimeObjectEntry> {
    runtime
        .object_table()
        .close_cleanup_entry_for_runtime(expected_kind, LedgerId(public_runtime_id))
}

pub fn lnb_public_id_for_live_object_result(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<i32, HalError> {
    let entry = aidl_object_entry_for_kind(runtime, object_id, generation, AidlObjectKind::Lnb)?;
    i32::try_from(entry.ledger_id.0).map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB runtime id is outside i32 range",
        )
    })
}

pub fn aidl_object_entry_for_close_cleanup(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<RuntimeObjectEntry, HalError> {
    let entry = runtime.object_table().entry(object_id).ok_or_else(|| {
        HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is missing",
        )
    })?;
    if entry.generation != generation {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object generation mismatch",
        ));
    }
    if entry.object_kind != expected_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch",
        ));
    }
    match entry.lifecycle {
        RuntimeObjectLifecycle::Live
        | RuntimeObjectLifecycle::Closing { .. }
        | RuntimeObjectLifecycle::CleanupFailed { .. } => Ok(entry.clone()),
        RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined => {
            Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object is terminal",
            ))
        }
    }
}

pub fn aidl_public_runtime_id_for_close_cleanup(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<i32, HalError> {
    let entry = aidl_object_entry_for_close_cleanup(runtime, object_id, generation, expected_kind)?;
    i32::try_from(entry.ledger_id.0).map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "AIDL object public runtime id is out of range",
        )
    })
}

#[cfg(test)]
mod closeable_lifecycle_tests {
    use super::*;
    use crate::{RuntimeObjectEntry, RuntimeOwnerRelation};
    use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration};

    fn runtime_with_filter() -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(501),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(501),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        runtime
    }

    #[test]
    fn aidl_object_closeable_accepts_live_and_cleanup_failed() {
        let mut runtime = runtime_with_filter();
        assert_eq!(
            aidl_object_closeable(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            )
            .expect("live object is closeable"),
            AidlObjectCloseability::BeginClose
        );

        runtime
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("begin close succeeds");
        runtime
            .object_table_mut()
            .mark_cleanup_failed_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("cleanup failed mark succeeds");

        assert_eq!(
            aidl_object_closeable(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            )
            .expect("cleanup failed object is closeable for retry"),
            AidlObjectCloseability::BeginClose
        );
    }

    #[test]
    fn aidl_object_closeable_rejects_closed() {
        let mut runtime = runtime_with_filter();
        runtime
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("begin close succeeds");
        runtime
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("commit close succeeds");
        assert!(aidl_object_closeable(
            &runtime,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());
    }

    #[test]
    fn aidl_object_closeable_rejects_closing_and_quarantined() {
        let mut closing = runtime_with_filter();
        closing
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("begin close succeeds");
        assert!(aidl_object_closeable(
            &closing,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());

        let mut quarantined = runtime_with_filter();
        quarantined
            .object_table_mut()
            .quarantine_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("quarantine succeeds");
        assert!(aidl_object_closeable(
            &quarantined,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());
    }

    #[test]
    fn aidl_object_closeable_rejects_closed_after_commit() {
        let mut closed = runtime_with_filter();
        closed
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("begin close succeeds");
        closed
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("commit close succeeds");
        assert!(aidl_object_closeable(
            &closed,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());
    }
}
