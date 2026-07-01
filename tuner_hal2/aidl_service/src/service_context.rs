use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
use binder::{Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
#[cfg(test)]
use maleicacid_tuner_hal2_binder_adapter::AidlApi;
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::{
    CallbackArtifactResetCommand, CallbackArtifactRuntimeSplitDiagnosticRecord,
    CallbackArtifactRuntimeSplitOutcome, FrontendProbeOutcome,
    OwnerCallbackCleanupArtifactCommand, ServiceBootOutcome, TunerServiceRuntime,
};

use crate::callback_store::{AidlCallbackStoreError, CallbackStore};
use crate::dvr_callback_delivery::{DvrStatusNotifier, DvrStatusNotifierKey};
use crate::object_handle::AidlObjectHandle;

const MAX_DROP_LEAK_ERROR_RECORDS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DropLeakErrorRecord {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    status_debug: String,
}

pub(crate) type SharedTunerRuntime = Arc<Mutex<TunerServiceRuntime>>;
pub type SharedAidlServiceContext = Arc<AidlServiceContext>;

pub struct AidlServiceContext {
    runtime: SharedTunerRuntime,
    callback_store: Mutex<CallbackStore>,
    dvr_status_notifiers: Mutex<BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>>,
    drop_leak_error_records: Mutex<VecDeque<DropLeakErrorRecord>>,
    drop_leak_error_records_dropped: AtomicUsize,
    drop_leak_error_record_failures: AtomicUsize,
}

impl AidlServiceContext {
    pub fn new(runtime: TunerServiceRuntime) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
            callback_store: Mutex::new(CallbackStore::default()),
            dvr_status_notifiers: Mutex::new(BTreeMap::new()),
            drop_leak_error_records: Mutex::new(VecDeque::new()),
            drop_leak_error_records_dropped: AtomicUsize::new(0),
            drop_leak_error_record_failures: AtomicUsize::new(0),
        }
    }

    pub fn shared(runtime: TunerServiceRuntime) -> SharedAidlServiceContext {
        Arc::new(Self::new(runtime))
    }

    #[cfg(test)]
    pub(crate) fn from_shared_runtime_for_test(
        runtime: SharedTunerRuntime,
    ) -> SharedAidlServiceContext {
        Arc::new(Self {
            runtime,
            callback_store: Mutex::new(CallbackStore::default()),
            dvr_status_notifiers: Mutex::new(BTreeMap::new()),
            drop_leak_error_records: Mutex::new(VecDeque::new()),
            drop_leak_error_records_dropped: AtomicUsize::new(0),
            drop_leak_error_record_failures: AtomicUsize::new(0),
        })
    }

    pub fn reset_runtime_from_probe_results<I>(
        &self,
        results: I,
    ) -> Result<ServiceBootOutcome, HalError>
    where
        I: IntoIterator<Item = FrontendProbeOutcome>,
    {
        crate::dvr_callback_delivery::stop_all_dvr_status_notifiers(self)?;
        let mut runtime = self.runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while planning callback artifact reset",
            )
        })?;
        let callback_reset_command = runtime.plan_callback_artifact_reset_before_boot_use_case();
        let artifact_result = self.clear_callback_artifact_reset_bridge(&callback_reset_command);
        let drop_leak_result = self.clear_drop_leak_error_records();
        let outcome = runtime.boot_from_probe_results(results);
        for split_outcome in CallbackArtifactRuntimeSplitOutcome::service_boot_reset_from_attempt_results(
            artifact_result.clone(),
            drop_leak_result.clone(),
            Ok(()),
        ) {
            runtime.record_callback_artifact_runtime_split_diagnostic(
                CallbackArtifactRuntimeSplitDiagnosticRecord::service_boot_reset(split_outcome),
            );
        }
        match (artifact_result, drop_leak_result) {
            (Ok(()), Ok(())) => Ok(outcome),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(primary), Err(cleanup)) => Err(maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                "service boot callback artifact/drop-leak reset failed",
                primary,
                cleanup,
            )),
        }
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

    pub(crate) fn record_drop_leak_error(&self, handle: AidlObjectHandle, status: &Status) {
        let record = DropLeakErrorRecord {
            object_kind: handle.object_kind(),
            object_id: handle.object_id(),
            generation: handle.generation(),
            status_debug: format!("{status:?}"),
        };
        let Ok(mut records) = self.drop_leak_error_records.lock() else {
            self.drop_leak_error_record_failures
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        if records.len() >= MAX_DROP_LEAK_ERROR_RECORDS {
            records.pop_front();
            self.drop_leak_error_records_dropped
                .fetch_add(1, Ordering::Relaxed);
        }
        records.push_back(record);
    }

    fn clear_drop_leak_error_records(&self) -> Result<(), HalError> {
        let mut records = self.drop_leak_error_records.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "drop-leak diagnostic store lock poisoned while clearing records",
            )
        })?;
        records.clear();
        self.drop_leak_error_records_dropped
            .store(0, Ordering::Relaxed);
        self.drop_leak_error_record_failures
            .store(0, Ordering::Relaxed);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_record_count(&self) -> usize {
        self.drop_leak_error_records
            .lock()
            .expect("drop-leak diagnostic store lock poisoned in test")
            .len()
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_records_dropped_count(&self) -> usize {
        self.drop_leak_error_records_dropped.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn drop_leak_error_record_failure_count(&self) -> usize {
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
    ) -> Result<(), HalError> {
        let handle = AidlObjectHandle::new(
            command.owner_kind(),
            command.owner_id(),
            command.owner_generation(),
        );
        self.clear_owner_callbacks_raw(handle)
            .map(|_| ())
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
