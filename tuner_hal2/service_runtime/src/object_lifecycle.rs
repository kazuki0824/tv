use crate::{
    error_mapping::object_table_error_to_hal, RuntimeObjectEntry, RuntimeObjectLifecycle,
    TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidlObjectCloseability {
    BeginClose,
}

fn invalid_close_lifecycle(lifecycle: RuntimeObjectLifecycle) -> HalError {
    let detail = match lifecycle {
        RuntimeObjectLifecycle::Prepared => {
            "AIDL objectはPreparedでありLiveとして公開されていません"
        }
        RuntimeObjectLifecycle::Closed => "AIDL objectは既にClosedです",
        RuntimeObjectLifecycle::Quarantined => {
            "AIDL objectはQuarantinedでありfail-closed処理が必要です"
        }
        RuntimeObjectLifecycle::Live
        | RuntimeObjectLifecycle::Closing { .. }
        | RuntimeObjectLifecycle::CleanupPending { .. } => "AIDL objectはcloseできません",
    };
    HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, detail)
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
        RuntimeObjectLifecycle::Live
        | RuntimeObjectLifecycle::Closing { .. }
        | RuntimeObjectLifecycle::CleanupPending { .. } => Ok(AidlObjectCloseability::BeginClose),
        RuntimeObjectLifecycle::Prepared
        | RuntimeObjectLifecycle::Closed
        | RuntimeObjectLifecycle::Quarantined => Err(invalid_close_lifecycle(entry.lifecycle)),
    }
}

/// Cleanup受付で使用するread-onlyなobject世代・状態分類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidlObjectCleanupDisposition {
    Stale,
    Closed,
    Quarantined,
    NoCleanupPending,
    RequiresIsolation,
}

pub fn aidl_object_cleanup_disposition(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<AidlObjectCleanupDisposition, HalError> {
    let Some(entry) = runtime.object_table().entry(object_id) else {
        return Ok(AidlObjectCleanupDisposition::Stale);
    };
    if entry.generation != generation {
        return Ok(AidlObjectCleanupDisposition::Stale);
    }
    if entry.object_kind != expected_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch while classifying cleanup retry",
        ));
    }
    Ok(match entry.lifecycle {
        RuntimeObjectLifecycle::Closed => AidlObjectCleanupDisposition::Closed,
        RuntimeObjectLifecycle::Quarantined => AidlObjectCleanupDisposition::Quarantined,
        RuntimeObjectLifecycle::Prepared | RuntimeObjectLifecycle::Live => {
            AidlObjectCleanupDisposition::NoCleanupPending
        }
        RuntimeObjectLifecycle::Closing { .. } | RuntimeObjectLifecycle::CleanupPending { .. } => {
            AidlObjectCleanupDisposition::RequiresIsolation
        }
    })
}

pub fn aidl_object_cleanup_dependency(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<CleanupStep, HalError> {
    let entry = runtime.object_table().entry(object_id).ok_or_else(|| {
        HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is missing while resolving cleanup dependency",
        )
    })?;
    if entry.generation != generation {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object generation mismatch while resolving cleanup dependency",
        ));
    }
    if entry.object_kind != expected_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch while resolving cleanup dependency",
        ));
    }
    match entry.lifecycle {
        RuntimeObjectLifecycle::Closing { step }
        | RuntimeObjectLifecycle::CleanupPending { step } => Ok(step),
        RuntimeObjectLifecycle::Prepared
        | RuntimeObjectLifecycle::Live
        | RuntimeObjectLifecycle::Closed
        | RuntimeObjectLifecycle::Quarantined => Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object has no pending cleanup dependency",
        )),
    }
}

pub fn aidl_object_cleanup_is_terminal(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    expected_kind: AidlObjectKind,
) -> Result<bool, HalError> {
    let entry = runtime.object_table().entry(object_id).ok_or_else(|| {
        HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object is missing while checking cleanup terminal state",
        )
    })?;
    if entry.generation != generation {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object generation mismatch while checking cleanup terminal state",
        ));
    }
    if entry.object_kind != expected_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL object kind mismatch while checking cleanup terminal state",
        ));
    }
    Ok(matches!(
        entry.lifecycle,
        RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
    ))
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
        | RuntimeObjectLifecycle::CleanupPending { .. } => Ok(entry.clone()),
        RuntimeObjectLifecycle::Prepared
        | RuntimeObjectLifecycle::Closed
        | RuntimeObjectLifecycle::Quarantined => Err(invalid_close_lifecycle(entry.lifecycle)),
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
    use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

    fn runtime_with_filter() -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                last_reference_ended: false,
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
    fn cleanup_disposition_classifies_stale_live_terminal_and_wrong_kind() {
        let mut runtime = runtime_with_filter();
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(999),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::Stale)
        );
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(2),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::Stale)
        );
        assert!(aidl_object_cleanup_disposition(
            &runtime,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Dvr,
        )
        .is_err());
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::NoCleanupPending)
        );
        runtime
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("close開始で後片付け依存関係を生成する");
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::RequiresIsolation)
        );
        runtime
            .object_table_mut()
            .quarantine_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("論理隔離に成功する");
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::Quarantined)
        );
    }

    #[test]
    fn prepared_object_does_not_have_cleanup_retry_obligation() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .object_table_mut()
            .insert_prepared(RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(502),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(502),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("Preparedの作成に成功");
        assert_eq!(
            aidl_object_cleanup_disposition(
                &runtime,
                AidlObjectId(502),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            ),
            Ok(AidlObjectCleanupDisposition::NoCleanupPending)
        );
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
            .expect("close開始が成功する");
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

    fn invalid_state_detail(error: HalError) -> String {
        match error {
            HalError::InvalidState { detail, .. } => detail.detail,
            other => panic!("InvalidStateを期待しましたが{other:?}でした"),
        }
    }

    fn assert_closeable_state_detail(runtime: &TunerServiceRuntime, expected: &str) {
        let error = aidl_object_closeable(
            runtime,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .expect_err("状態がclose可能ではありません");
        assert_eq!(invalid_state_detail(error), expected);
    }

    #[test]
    fn aidl_object_closeable_reports_state_without_collapsing_diagnostics() {
        let mut prepared = TunerServiceRuntime::new();
        prepared
            .object_table_mut()
            .insert_prepared(RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(501),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(501),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("Prepared objectのinsertが成功する");
        assert_closeable_state_detail(
            &prepared,
            "AIDL objectはPreparedでありLiveとして公開されていません",
        );

        let mut closed = runtime_with_filter();
        closed
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("close開始が成功する");
        closed
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("close commitが成功する");
        assert_closeable_state_detail(&closed, "AIDL objectは既にClosedです");

        let mut quarantined = runtime_with_filter();
        quarantined
            .object_table_mut()
            .quarantine_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("quarantineが成功する");
        assert_closeable_state_detail(
            &quarantined,
            "AIDL objectはQuarantinedでありfail-closed処理が必要です",
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
            .expect("close開始が成功する");
        runtime
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("close commitが成功する");
        assert!(aidl_object_closeable(
            &runtime,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());
    }

    #[test]
    fn aidl_object_closeable_accepts_closing_but_rejects_quarantined() {
        let mut closing = runtime_with_filter();
        closing
            .object_table_mut()
            .begin_close_cascade(
                AidlObjectId(501),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("close開始が成功する");
        assert_eq!(
            aidl_object_closeable(
                &closing,
                AidlObjectId(501),
                AidlObjectGeneration(1),
                AidlObjectKind::Filter,
            )
            .expect("closing object remains closeable when cleanup authority was dropped"),
            AidlObjectCloseability::BeginClose
        );

        let mut quarantined = runtime_with_filter();
        quarantined
            .object_table_mut()
            .quarantine_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("quarantineが成功する");
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
            .expect("close開始が成功する");
        closed
            .object_table_mut()
            .commit_close_cascade(AidlObjectId(501), AidlObjectGeneration(1))
            .expect("close commitが成功する");
        assert!(aidl_object_closeable(
            &closed,
            AidlObjectId(501),
            AidlObjectGeneration(1),
            AidlObjectKind::Filter,
        )
        .is_err());
    }
}
