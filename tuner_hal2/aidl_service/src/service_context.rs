use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
use binder::{Status, Strong};
#[cfg(test)]
use maleicacid_tuner_hal2_binder_adapter::AidlApi;
use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
#[cfg(test)]
use maleicacid_tuner_hal2_service_runtime::DiagnosticSnapshot;
use maleicacid_tuner_hal2_service_runtime::{
    BoundedDiagnosticStore, CallbackArtifactCleanupResult, CallbackArtifactResetCommand,
    CallbackArtifactRuntimeSplitDiagnosticRecord, CallbackArtifactRuntimeSplitOutcome,
    FilterCallbackDeliveryDiagnosticRecord, FilterCallbackDeliveryDiagnosticSnapshot,
    FrontendCallbackDeliveryDiagnosticRecord, FrontendCallbackDeliveryDiagnosticSnapshot,
    FrontendProbeOutcome, ObjectCleanupDiagnosticRecord, OwnerCallbackCleanupArtifactCommand,
    ServiceBootOutcome, SharedCallbackArtifactRuntimeSplitDiagnostics,
    SharedDvrPostCommitNotificationDiagnostics, SharedDvrStatusNotifierCleanupDiagnostics,
    SharedObjectCleanupDiagnostics, TunerServiceRuntime,
};

use crate::callback_store::{AidlCallbackStoreError, CallbackStore};
use crate::dvr_callback_delivery::{DvrStatusNotifier, DvrStatusNotifierKey};
use crate::object_handle::AidlObjectHandle;

const MAX_DROP_LEAK_ERROR_RECORDS: usize = 64;

fn saturating_increment_atomic_usize(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DropLeakStatusSnapshot {
    exception_code: String,
    service_specific_error_code: i32,
    transaction_error: String,
    message: Option<String>,
    debug_fallback: String,
}

impl DropLeakStatusSnapshot {
    fn from_status(status: &Status) -> Self {
        let description = status.get_description();
        Self {
            exception_code: format!("{:?}", status.exception_code()),
            service_specific_error_code: status.service_specific_error(),
            transaction_error: format!("{:?}", status.transaction_error()),
            message: Some(description),
            debug_fallback: format!("{status:?}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DropLeakErrorRecord {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    status: DropLeakStatusSnapshot,
}

pub(crate) type SharedTunerRuntime = Arc<Mutex<TunerServiceRuntime>>;
pub type SharedAidlServiceContext = Arc<AidlServiceContext>;

pub struct AidlServiceContext {
    runtime: SharedTunerRuntime,
    callback_store: Mutex<CallbackStore>,
    dvr_status_notifiers: Mutex<BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>>,
    drop_leak_error_records: Mutex<BoundedDiagnosticStore<DropLeakErrorRecord>>,
    drop_leak_error_record_failures: AtomicUsize,
    callback_artifact_runtime_split_diagnostics: SharedCallbackArtifactRuntimeSplitDiagnostics,
    dvr_post_commit_notification_diagnostics: SharedDvrPostCommitNotificationDiagnostics,
    dvr_status_notifier_cleanup_diagnostics: SharedDvrStatusNotifierCleanupDiagnostics,
    object_cleanup_diagnostics: SharedObjectCleanupDiagnostics,
    filter_callback_delivery_fallback_diagnostics:
        Mutex<BoundedDiagnosticStore<FilterCallbackDeliveryDiagnosticRecord>>,
    frontend_callback_delivery_fallback_diagnostics:
        Mutex<BoundedDiagnosticStore<FrontendCallbackDeliveryDiagnosticRecord>>,
    filter_callback_delivery_fallback_record_failures: AtomicUsize,
    frontend_callback_delivery_fallback_record_failures: AtomicUsize,
}

impl AidlServiceContext {
    pub fn new(runtime: TunerServiceRuntime) -> Self {
        let callback_artifact_runtime_split_diagnostics =
            runtime.callback_artifact_runtime_split_diagnostic_sink();
        let dvr_post_commit_notification_diagnostics =
            runtime.dvr_post_commit_notification_diagnostic_sink();
        let dvr_status_notifier_cleanup_diagnostics =
            runtime.dvr_status_notifier_cleanup_diagnostic_sink();
        let object_cleanup_diagnostics = runtime.object_cleanup_diagnostic_sink();
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
            callback_store: Mutex::new(CallbackStore::default()),
            dvr_status_notifiers: Mutex::new(BTreeMap::new()),
            drop_leak_error_records: Mutex::new(BoundedDiagnosticStore::new(
                MAX_DROP_LEAK_ERROR_RECORDS,
            )),
            drop_leak_error_record_failures: AtomicUsize::new(0),
            callback_artifact_runtime_split_diagnostics,
            dvr_post_commit_notification_diagnostics,
            dvr_status_notifier_cleanup_diagnostics,
            object_cleanup_diagnostics,
            filter_callback_delivery_fallback_diagnostics: Mutex::new(
                BoundedDiagnosticStore::default(),
            ),
            frontend_callback_delivery_fallback_diagnostics: Mutex::new(
                BoundedDiagnosticStore::default(),
            ),
            filter_callback_delivery_fallback_record_failures: AtomicUsize::new(0),
            frontend_callback_delivery_fallback_record_failures: AtomicUsize::new(0),
        }
    }

    pub fn shared(runtime: TunerServiceRuntime) -> SharedAidlServiceContext {
        Arc::new(Self::new(runtime))
    }

    #[cfg(test)]
    pub(crate) fn from_shared_runtime_for_test(
        runtime: SharedTunerRuntime,
    ) -> SharedAidlServiceContext {
        let (
            callback_artifact_runtime_split_diagnostics,
            dvr_post_commit_notification_diagnostics,
            dvr_status_notifier_cleanup_diagnostics,
            object_cleanup_diagnostics,
        ) = {
            let runtime_guard = runtime
                .lock()
                .expect("service runtime lock poisoned while cloning diagnostic sinks");
            (
                runtime_guard.callback_artifact_runtime_split_diagnostic_sink(),
                runtime_guard.dvr_post_commit_notification_diagnostic_sink(),
                runtime_guard.dvr_status_notifier_cleanup_diagnostic_sink(),
                runtime_guard.object_cleanup_diagnostic_sink(),
            )
        };
        Arc::new(Self {
            runtime,
            callback_store: Mutex::new(CallbackStore::default()),
            dvr_status_notifiers: Mutex::new(BTreeMap::new()),
            drop_leak_error_records: Mutex::new(BoundedDiagnosticStore::new(
                MAX_DROP_LEAK_ERROR_RECORDS,
            )),
            drop_leak_error_record_failures: AtomicUsize::new(0),
            callback_artifact_runtime_split_diagnostics,
            dvr_post_commit_notification_diagnostics,
            dvr_status_notifier_cleanup_diagnostics,
            object_cleanup_diagnostics,
            filter_callback_delivery_fallback_diagnostics: Mutex::new(
                BoundedDiagnosticStore::default(),
            ),
            frontend_callback_delivery_fallback_diagnostics: Mutex::new(
                BoundedDiagnosticStore::default(),
            ),
            filter_callback_delivery_fallback_record_failures: AtomicUsize::new(0),
            frontend_callback_delivery_fallback_record_failures: AtomicUsize::new(0),
        })
    }

    pub fn reset_runtime_from_probe_results<I>(
        &self,
        results: I,
    ) -> Result<ServiceBootOutcome, HalError>
    where
        I: IntoIterator<Item = FrontendProbeOutcome>,
    {
        let dvr_notifier_result = crate::dvr_callback_delivery::stop_all_dvr_status_notifiers(self);
        let artifact_result = match self.runtime.lock() {
            Ok(runtime) => {
                let callback_reset_command =
                    runtime.plan_callback_artifact_reset_before_boot_use_case();
                self.clear_callback_artifact_reset_bridge(&callback_reset_command)
            }
            Err(_) => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while planning callback artifact reset",
            )),
        };
        let drop_leak_result = self.clear_drop_leak_error_records();
        let callback_fallback_clear_result = self.clear_callback_delivery_fallback_diagnostics();
        let mut runtime = match self.runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                let runtime_error = HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while finishing service boot reset",
                );
                let record_result = self.record_service_boot_reset_finish_lock_failure(
                    dvr_notifier_result.clone(),
                    artifact_result,
                    drop_leak_result,
                    callback_fallback_clear_result,
                    runtime_error.clone(),
                );
                return Err(match record_result {
                    Ok(()) => runtime_error,
                    Err(record_error) => compose_primary_cleanup_failure(
                        "service boot reset split diagnostic record failed after runtime finish lock failure",
                        runtime_error,
                        record_error,
                    ),
                });
            }
        };
        let (outcome, diagnostic_clear_result) =
            runtime.boot_from_probe_results_with_diagnostic_clear_result(results);
        runtime.finish_service_boot_reset_after_artifact_result_use_case(
            outcome,
            dvr_notifier_result,
            artifact_result,
            drop_leak_result,
            callback_fallback_clear_result,
            diagnostic_clear_result,
        )
    }

    pub(crate) fn runtime(&self) -> SharedTunerRuntime {
        Arc::clone(&self.runtime)
    }

    pub(crate) fn lock_runtime(
        &self,
    ) -> Result<MutexGuard<'_, TunerServiceRuntime>, binder::Status> {
        self.runtime
            .lock()
            .map_err(|_| crate::error_bridge::status_unknown_error("service runtime lock poisoned"))
    }

    pub(crate) fn record_callback_artifact_runtime_split_finish_lock_failure(
        &self,
        record: CallbackArtifactRuntimeSplitDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.callback_artifact_runtime_split_diagnostics
            .record(record)
    }

    pub(crate) fn record_dvr_post_commit_notification_diagnostic_fallback(
        &self,
        record: maleicacid_tuner_hal2_service_runtime::DvrPostCommitNotificationDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.dvr_post_commit_notification_diagnostics.record(record)
    }

    pub(crate) fn record_dvr_status_notifier_cleanup_diagnostic(
        &self,
        record: maleicacid_tuner_hal2_service_runtime::DvrStatusNotifierCleanupDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.dvr_status_notifier_cleanup_diagnostics.record(record)
    }

    pub(crate) fn record_object_cleanup_diagnostic_fallback(
        &self,
        record: ObjectCleanupDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.object_cleanup_diagnostics.record(record)
    }

    pub(crate) fn record_filter_callback_delivery_failure_fallback(
        &self,
        record: FilterCallbackDeliveryDiagnosticRecord,
    ) -> Result<(), HalError> {
        match self.filter_callback_delivery_fallback_diagnostics.lock() {
            Ok(mut diagnostics) => {
                diagnostics.push(record);
                Ok(())
            }
            Err(_) => {
                saturating_increment_atomic_usize(
                    &self.filter_callback_delivery_fallback_record_failures,
                );
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter callback delivery fallback diagnostic store lock poisoned",
                ))
            }
        }
    }

    pub(crate) fn record_frontend_callback_delivery_failure_fallback(
        &self,
        record: FrontendCallbackDeliveryDiagnosticRecord,
    ) -> Result<(), HalError> {
        match self.frontend_callback_delivery_fallback_diagnostics.lock() {
            Ok(mut diagnostics) => {
                diagnostics.push(record);
                Ok(())
            }
            Err(_) => {
                saturating_increment_atomic_usize(
                    &self.frontend_callback_delivery_fallback_record_failures,
                );
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend callback delivery fallback diagnostic store lock poisoned",
                ))
            }
        }
    }

    pub fn filter_callback_delivery_diagnostic_snapshot(
        &self,
    ) -> Result<FilterCallbackDeliveryDiagnosticSnapshot, HalError> {
        let mut records = Vec::new();
        let mut dropped_count = 0u64;
        let runtime_snapshot_missing = match self.runtime.lock() {
            Ok(runtime) => {
                let runtime_snapshot = runtime.filter_callback_delivery_diagnostic_snapshot();
                records.extend_from_slice(runtime_snapshot.records());
                dropped_count = dropped_count.saturating_add(runtime_snapshot.dropped_count());
                false
            }
            Err(_) => true,
        };
        let fallback = self.filter_callback_delivery_fallback_diagnostics.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter callback delivery fallback diagnostic store lock poisoned while snapshotting",
            )
        })?;
        let fallback_record_count = fallback.as_slice().len();
        let fallback_dropped_count = fallback.dropped_count();
        records.extend_from_slice(fallback.as_slice());
        dropped_count = dropped_count.saturating_add(fallback_dropped_count);
        Ok(FilterCallbackDeliveryDiagnosticSnapshot::new_with_metadata(
            records,
            dropped_count,
            runtime_snapshot_missing,
            fallback_record_count,
            fallback_dropped_count,
            self.filter_callback_delivery_fallback_record_failure_count() as u64,
        ))
    }

    pub fn frontend_callback_delivery_diagnostic_snapshot(
        &self,
    ) -> Result<FrontendCallbackDeliveryDiagnosticSnapshot, HalError> {
        let mut records = Vec::new();
        let mut dropped_count = 0u64;
        let runtime_snapshot_missing = match self.runtime.lock() {
            Ok(runtime) => {
                let runtime_snapshot = runtime.frontend_callback_delivery_diagnostic_snapshot();
                records.extend_from_slice(runtime_snapshot.records());
                dropped_count = dropped_count.saturating_add(runtime_snapshot.dropped_count());
                false
            }
            Err(_) => true,
        };
        let fallback = self.frontend_callback_delivery_fallback_diagnostics.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend callback delivery fallback diagnostic store lock poisoned while snapshotting",
            )
        })?;
        let fallback_record_count = fallback.as_slice().len();
        let fallback_dropped_count = fallback.dropped_count();
        records.extend_from_slice(fallback.as_slice());
        dropped_count = dropped_count.saturating_add(fallback_dropped_count);
        Ok(
            FrontendCallbackDeliveryDiagnosticSnapshot::new_with_metadata(
                records,
                dropped_count,
                runtime_snapshot_missing,
                fallback_record_count,
                fallback_dropped_count,
                self.frontend_callback_delivery_fallback_record_failure_count() as u64,
            ),
        )
    }

    pub fn filter_callback_delivery_fallback_record_failure_count(&self) -> usize {
        self.filter_callback_delivery_fallback_record_failures
            .load(Ordering::Relaxed)
    }

    pub fn frontend_callback_delivery_fallback_record_failure_count(&self) -> usize {
        self.frontend_callback_delivery_fallback_record_failures
            .load(Ordering::Relaxed)
    }

    fn clear_callback_delivery_fallback_diagnostics(&self) -> Result<(), HalError> {
        let mut failures = maleicacid_tuner_hal2_common::FirstErrorCollector::new();
        match self.filter_callback_delivery_fallback_diagnostics.lock() {
            Ok(mut diagnostics) => {
                diagnostics.clear();
                self.filter_callback_delivery_fallback_record_failures
                    .store(0, Ordering::Relaxed);
            }
            Err(_) => failures.push_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter callback delivery fallback diagnostic store lock poisoned while clearing",
            )),
        }
        match self.frontend_callback_delivery_fallback_diagnostics.lock() {
            Ok(mut diagnostics) => {
                diagnostics.clear();
                self.frontend_callback_delivery_fallback_record_failures
                    .store(0, Ordering::Relaxed);
            }
            Err(_) => failures.push_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend callback delivery fallback diagnostic store lock poisoned while clearing",
            )),
        }
        failures.into_result()
    }

    pub(crate) fn record_service_boot_reset_finish_lock_failure(
        &self,
        dvr_notifier_result: Result<(), HalError>,
        artifact_result: Result<(), HalError>,
        drop_leak_result: Result<(), HalError>,
        callback_fallback_clear_result: Result<(), HalError>,
        runtime_error: HalError,
    ) -> Result<(), HalError> {
        let mut record_error: Option<HalError> = None;
        for outcome in CallbackArtifactRuntimeSplitOutcome::service_boot_reset_from_attempt_results(
            dvr_notifier_result,
            artifact_result,
            drop_leak_result,
            callback_fallback_clear_result,
            Ok(()),
            Err(runtime_error),
        ) {
            if let Err(error) = self.record_callback_artifact_runtime_split_finish_lock_failure(
                CallbackArtifactRuntimeSplitDiagnosticRecord::service_boot_reset(outcome),
            ) {
                record_error = Some(match record_error {
                    Some(primary) => compose_primary_cleanup_failure(
                        "service boot split diagnostic record failed repeatedly after runtime finish lock failure",
                        primary,
                        error,
                    ),
                    None => error,
                });
            }
        }
        match record_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn callback_store_lock(
        &self,
    ) -> Result<MutexGuard<'_, CallbackStore>, AidlCallbackStoreError> {
        self.callback_store
            .lock()
            .map_err(|_| AidlCallbackStoreError::Poisoned)
    }

    pub(crate) fn dvr_status_notifiers_lock(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>>, HalError> {
        self.dvr_status_notifiers.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier store lock poisoned",
            )
        })
    }

    pub(crate) fn take_dvr_status_notifiers_for_reset(
        &self,
    ) -> (
        BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>,
        Result<(), HalError>,
    ) {
        match self.dvr_status_notifiers.lock() {
            Ok(mut store) => (std::mem::take(&mut *store), Ok(())),
            Err(poisoned) => {
                let mut store = poisoned.into_inner();
                (
                    std::mem::take(&mut *store),
                    Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "DVR status notifier store lock poisoned while taking notifiers for reset",
                    )),
                )
            }
        }
    }

    pub(crate) fn record_drop_leak_error(&self, handle: AidlObjectHandle, status: &Status) {
        let record = DropLeakErrorRecord {
            object_kind: handle.object_kind(),
            object_id: handle.object_id(),
            generation: handle.generation(),
            status: DropLeakStatusSnapshot::from_status(status),
        };
        let Ok(mut records) = self.drop_leak_error_records.lock() else {
            saturating_increment_atomic_usize(&self.drop_leak_error_record_failures);
            return;
        };
        records.push(record);
    }

    fn clear_drop_leak_error_records(&self) -> Result<(), HalError> {
        let mut records = self.drop_leak_error_records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "drop-leak diagnostic store lock poisoned while clearing records",
            )
        })?;
        records.clear();
        self.drop_leak_error_record_failures
            .store(0, Ordering::Relaxed);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_diagnostic_snapshot(
        &self,
    ) -> Result<DiagnosticSnapshot<DropLeakErrorRecord>, HalError> {
        let records = self.drop_leak_error_records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "drop-leak diagnostic store lock poisoned while snapshotting records",
            )
        })?;
        Ok(DiagnosticSnapshot::new(
            records.as_slice().to_vec(),
            records.dropped_count(),
        ))
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_record_count(&self) -> usize {
        self.drop_leak_error_diagnostic_snapshot()
            .expect("drop-leak diagnostic snapshot failed in test")
            .records()
            .len()
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_records_dropped_count(&self) -> usize {
        self.drop_leak_error_diagnostic_snapshot()
            .expect("drop-leak diagnostic snapshot failed in test")
            .dropped_count() as usize
    }

    pub fn drop_leak_error_record_failure_count(&self) -> usize {
        self.drop_leak_error_record_failures.load(Ordering::Relaxed)
    }

    pub(crate) fn retain_frontend_callback(
        &self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFrontendCallback>,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .retain_frontend_callback(handle, callback);
        Ok(())
    }

    pub(crate) fn retain_lnb_callback(
        &self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn ILnbCallback>,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .retain_lnb_callback(handle, callback);
        Ok(())
    }

    pub(crate) fn retain_filter_callback(
        &self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFilterCallback>,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .retain_filter_callback(handle, callback);
        Ok(())
    }

    pub(crate) fn retain_dvr_callback(
        &self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IDvrCallback>,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .retain_dvr_callback(handle, callback);
        Ok(())
    }

    fn clear_owner_callbacks_raw(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<usize, AidlCallbackStoreError> {
        Ok(self.callback_store_lock()?.clear_owner_callbacks(handle))
    }

    #[cfg(test)]
    pub(crate) fn clear_owner_callbacks_for_test(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<usize, AidlCallbackStoreError> {
        self.clear_owner_callbacks_raw(handle)
    }

    fn clear_all_callback_artifacts_raw(&self) -> Result<usize, AidlCallbackStoreError> {
        Ok(self.callback_store_lock()?.clear_all_callbacks())
    }

    pub(crate) fn clear_callback_artifact_reset_bridge(
        &self,
        command: &CallbackArtifactResetCommand,
    ) -> Result<(), HalError> {
        self.clear_all_callback_artifacts_raw()
            .map(|_| ())
            .map_err(|error| error.into_hal_error(command.failure_message()))
    }

    pub(crate) fn clear_owner_callback_artifacts_bridge(
        &self,
        command: &OwnerCallbackCleanupArtifactCommand,
    ) -> Result<CallbackArtifactCleanupResult, HalError> {
        let handle = AidlObjectHandle::new(
            command.owner_kind(),
            command.owner_id(),
            command.owner_generation(),
        );
        self.clear_owner_callbacks_raw(handle)
            .map(|removed| {
                if removed == 0 {
                    CallbackArtifactCleanupResult::NoArtifact
                } else {
                    CallbackArtifactCleanupResult::Cleared
                }
            })
            .map_err(|error| error.into_hal_error(command.cleanup_failure_message()))
    }

    pub(crate) fn frontend_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<Option<Strong<dyn IFrontendCallback>>, AidlCallbackStoreError> {
        Ok(self
            .callback_store_lock()?
            .frontend_callback_for_owner(handle))
    }

    pub(crate) fn filter_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<Option<Strong<dyn IFilterCallback>>, AidlCallbackStoreError> {
        Ok(self
            .callback_store_lock()?
            .filter_callback_for_owner(handle))
    }

    pub(crate) fn dvr_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<Option<Strong<dyn IDvrCallback>>, AidlCallbackStoreError> {
        Ok(self.callback_store_lock()?.dvr_callback_for_owner(handle))
    }

    #[cfg(test)]
    pub(crate) fn has_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
    ) -> Result<bool, AidlCallbackStoreError> {
        Ok(self
            .callback_store_lock()?
            .has_callback_for_owner(handle, api))
    }

    #[cfg(test)]
    pub(crate) fn retain_test_callback_marker(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .retain_test_callback_marker(handle, api);
        Ok(())
    }
}
