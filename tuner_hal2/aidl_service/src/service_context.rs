use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
use binder::{Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::{
    object_lifecycle::aidl_object_for_close_cleanup_runtime, CallbackRegistryUpdate,
    FrontendProbeOutcome, ServiceBootOutcome, TunerServiceRuntime,
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
        self.clear_all_callback_artifacts().map_err(|error| {
            error.into_hal_error("callback artifact reset failed before runtime boot")
        })?;
        self.clear_drop_leak_error_records()?;
        let mut runtime = self.runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while resetting runtime from probe results",
            )
        })?;
        Ok(runtime.boot_from_probe_results(results))
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

    pub(crate) fn clear_owner_callbacks(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<usize, AidlCallbackStoreError> {
        Ok(self.callback_store_lock()?.clear_owner_callbacks(handle))
    }

    pub(crate) fn clear_all_callback_artifacts(&self) -> Result<usize, AidlCallbackStoreError> {
        Ok(self.callback_store_lock()?.clear_all_callbacks())
    }

    pub(crate) fn clear_lnb_owner_loss_callback_for_public_id(
        &self,
        lnb_id: i32,
    ) -> Result<(), HalError> {
        let handle = {
            let runtime = self.runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while resolving LNB callback owner",
                )
            })?;
            let Some(entry) = aidl_object_for_close_cleanup_runtime(
                &runtime,
                AidlObjectKind::Lnb,
                i64::from(lnb_id),
            ) else {
                return Err(HalError::cleanup_failed(
                    "LNB owner-loss callback cleanup",
                    format!("LNB AIDL object is missing during owner-loss cleanup: id={lnb_id}"),
                ));
            };
            AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
        };

        match self.clear_owner_callbacks(handle) {
            Ok(_) => {
                let mut runtime = self.runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while clearing LNB callback registry",
                    )
                })?;
                match runtime
                    .clear_callback_registration_owner(handle.object_id(), handle.generation())
                {
                    CallbackRegistryUpdate::Updated => Ok(()),
                    CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
                        "LNB owner-loss callback cleanup",
                        "callback registry owner missing while clearing LNB callback",
                    )),
                }
            }
            Err(error) => {
                let cleanup_error =
                    error.into_hal_error("callback store cleanup failed during LNB owner loss");
                let mut runtime = match self.runtime.lock() {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        let mark_error = HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while marking LNB callback unhealthy",
                        );
                        return Err(compose_primary_cleanup_failure(
                            "LNB owner-loss callback cleanup failed and unhealthy marking failed",
                            cleanup_error,
                            mark_error,
                        ));
                    }
                };
                let mark_result = runtime.mark_callback_registration_unhealthy(
                    AidlObjectKind::Lnb,
                    handle.object_id(),
                    handle.generation(),
                    AidlApi::LnbSetCallback,
                );
                match mark_result {
                    CallbackRegistryUpdate::Updated => Err(cleanup_error),
                    CallbackRegistryUpdate::Missing => Err(compose_primary_cleanup_failure(
                        "LNB owner-loss callback cleanup failed and unhealthy marking failed",
                        cleanup_error,
                        HalError::cleanup_failed(
                            "LNB owner-loss callback cleanup",
                            "callback registry owner missing while marking unhealthy",
                        ),
                    )),
                }
            }
        }
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
