from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


def replace_all_checked(path: str, old: str, new: str, expected: int) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} anchors, got {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new))


store = "tuner_hal2/aidl_service/src/callback_store.rs"
replace_once(
    store,
    "    prepared_callbacks: BTreeMap<CallbackStoreKey, (PreparedCallbackArtifactToken, StoredCallback)>,\n",
    "    prepared_callbacks: BTreeMap<CallbackStoreKey, (u64, StoredCallback)>,\n",
)
replace_once(
    store,
    "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken(u64);\n",
    "#[derive(Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken(u64);\n",
)
# Four production prepare paths plus the test marker path: the store retains only
# the immutable identity, while the returned wrapper remains a unique authority.
replace_all_checked(store, "            (token, StoredCallback::", "            (token.0, StoredCallback::", 5)

replace_once(
    store,
    '''    pub(crate) fn commit_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> bool {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token)
        {
            return false;
        }
        let Some((_, callback)) = self.prepared_callbacks.remove(&key) else {
            return false;
        };
        self.callbacks.insert(key, callback);
        true
    }

    pub(crate) fn abort_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> bool {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token)
        {
            return false;
        }
        self.prepared_callbacks.remove(&key).is_some()
    }
''',
    '''    pub(crate) fn commit_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<(), AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token.0)
        {
            return Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch);
        }
        let (_, callback) = self
            .prepared_callbacks
            .remove(&key)
            .ok_or(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)?;
        self.callbacks.insert(key, callback);
        Ok(())
    }

    pub(crate) fn abort_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<(), AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token.0)
        {
            return Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch);
        }
        self.prepared_callbacks
            .remove(&key)
            .map(|_| ())
            .ok_or(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)
    }
''',
)

replace_once(
    store,
    '''        assert!(store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
''',
    '''        assert_eq!(
            store.abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
''',
)
replace_once(
    store,
    '''        assert!(store.commit_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
''',
    '''        assert_eq!(
            store.commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
''',
)
replace_once(
    store,
    '''        let stale = PreparedCallbackArtifactToken(token.0 + 1);

        assert!(!store.commit_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            stale
        ));
        assert!(!store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            stale
        ));
        assert!(store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
''',
    '''        let stale_id = token.0 + 1;

        assert_eq!(
            store.commit_prepared_callback(
                handle,
                AidlApi::FrontendSetCallback,
                PreparedCallbackArtifactToken(stale_id),
            ),
            Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)
        );
        assert_eq!(
            store.abort_prepared_callback(
                handle,
                AidlApi::FrontendSetCallback,
                PreparedCallbackArtifactToken(stale_id),
            ),
            Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)
        );
        assert_eq!(
            store.abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
''',
)
replace_once(
    store,
    '''pub(crate) enum AidlCallbackStoreError {
    Poisoned,
    PreparedArtifactInFlight,
    PreparedTokenExhausted,
}
''',
    '''pub(crate) enum AidlCallbackStoreError {
    Poisoned,
    PreparedArtifactInFlight,
    PreparedArtifactAuthorityMismatch,
    PreparedTokenExhausted,
}
''',
)
replace_once(
    store,
    '''            Self::PreparedTokenExhausted => HalError::internal(
''',
    '''            Self::PreparedArtifactAuthorityMismatch => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: prepared callback artifact authority is stale or missing"),
            ),
            Self::PreparedTokenExhausted => HalError::internal(
''',
)

context = "tuner_hal2/aidl_service/src/service_context.rs"
# Generic callback registration bridge: consume the one-shot authority and
# preserve typed store errors rather than wrapping them in a bool.
replace_once(
    context,
    '''    pub(crate) fn commit_prepared_callback(
        &self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<bool, AidlCallbackStoreError> {
        Ok(self
            .callback_store_lock()?
            .commit_prepared_callback(handle, registration_api, token))
    }

    pub(crate) fn abort_prepared_callback(
        &self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<bool, AidlCallbackStoreError> {
        Ok(self
            .callback_store_lock()?
            .abort_prepared_callback(handle, registration_api, token))
    }
''',
    '''    pub(crate) fn commit_prepared_callback(
        &self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .commit_prepared_callback(handle, registration_api, token)
    }

    pub(crate) fn abort_prepared_callback(
        &self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<(), AidlCallbackStoreError> {
        self.callback_store_lock()?
            .abort_prepared_callback(handle, registration_api, token)
    }
''',
)
# The child-open helper introduced after the generic bridge must use the actual
# callback_store owner, not a shadow/nonexistent field, and must not swallow an
# abort mismatch.
replace_once(
    context,
    '''    pub(crate) fn prepare_filter_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        callback: &binder::Strong<dyn android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFilterCallback::IFilterCallback>,
    ) -> Result<crate::callback_store::PreparedCallbackArtifactToken, HalError> {
        self.callbacks
            .lock()
            .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, "callback store lock poisoned"))?
            .prepare_filter_callback(handle, callback)
            .map_err(|error| error.into_hal_error("filter callback prepare failed"))
    }

    pub(crate) fn prepare_dvr_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        callback: &binder::Strong<dyn android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IDvrCallback::IDvrCallback>,
    ) -> Result<crate::callback_store::PreparedCallbackArtifactToken, HalError> {
        self.callbacks
            .lock()
            .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, "callback store lock poisoned"))?
            .prepare_dvr_callback(handle, callback)
            .map_err(|error| error.into_hal_error("DVR callback prepare failed"))
    }

    pub(crate) fn commit_child_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
        token: crate::callback_store::PreparedCallbackArtifactToken,
    ) -> Result<(), HalError> {
        let committed = self.callbacks
            .lock()
            .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, "callback store lock poisoned"))?
            .commit_prepared_callback(handle, api, token);
        if committed { Ok(()) } else { Err(HalError::internal(HalInternalKind::InvariantViolation, "prepared child callback disappeared before commit")) }
    }

    pub(crate) fn abort_child_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
        token: crate::callback_store::PreparedCallbackArtifactToken,
    ) -> Result<(), HalError> {
        let _ = self.callbacks
            .lock()
            .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, "callback store lock poisoned"))?
            .abort_prepared_callback(handle, api, token);
        Ok(())
    }
''',
    '''    pub(crate) fn prepare_filter_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        callback: &binder::Strong<dyn android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFilterCallback::IFilterCallback>,
    ) -> Result<crate::callback_store::PreparedCallbackArtifactToken, HalError> {
        self.callback_store_lock()
            .map_err(|error| error.into_hal_error("filter callback store lock failed during child prepare"))?
            .prepare_filter_callback(handle, callback)
            .map_err(|error| error.into_hal_error("filter callback prepare failed"))
    }

    pub(crate) fn prepare_dvr_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        callback: &binder::Strong<dyn android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IDvrCallback::IDvrCallback>,
    ) -> Result<crate::callback_store::PreparedCallbackArtifactToken, HalError> {
        self.callback_store_lock()
            .map_err(|error| error.into_hal_error("DVR callback store lock failed during child prepare"))?
            .prepare_dvr_callback(handle, callback)
            .map_err(|error| error.into_hal_error("DVR callback prepare failed"))
    }

    pub(crate) fn commit_child_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
        token: crate::callback_store::PreparedCallbackArtifactToken,
    ) -> Result<(), HalError> {
        self.callback_store_lock()
            .map_err(|error| error.into_hal_error("child callback store lock failed during commit"))?
            .commit_prepared_callback(handle, api, token)
            .map_err(|error| error.into_hal_error("prepared child callback commit failed"))
    }

    pub(crate) fn abort_child_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
        token: crate::callback_store::PreparedCallbackArtifactToken,
    ) -> Result<(), HalError> {
        self.callback_store_lock()
            .map_err(|error| error.into_hal_error("child callback store lock failed during abort"))?
            .abort_prepared_callback(handle, api, token)
            .map_err(|error| error.into_hal_error("prepared child callback abort failed"))
    }
''',
)

obj = "tuner_hal2/aidl_service/src/object_runtime/mod.rs"
replace_once(
    obj,
    '''    context
        .abort_prepared_callback(handle, registration_api, token)
        .map(|removed| {
            if removed {
                CallbackArtifactCleanupResult::Cleared
            } else {
                CallbackArtifactCleanupResult::NoArtifact
            }
        })
        .map_err(|error| error.into_hal_error(command.cleanup_failure_message()))
''',
    '''    context
        .abort_prepared_callback(handle, registration_api, token)
        .map(|()| CallbackArtifactCleanupResult::Cleared)
        .map_err(|error| error.into_hal_error(command.cleanup_failure_message()))
''',
)
replace_once(
    obj,
    '''    let committed = context
        .commit_prepared_callback(handle, registration_api, token)
        .map_err(|error| error.into_hal_error("prepared callback artifact commit failed"))?;
    if committed {
        Ok(())
    } else {
        Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "prepared callback artifact disappeared before composite commit",
        ))
    }
''',
    '''    context
        .commit_prepared_callback(handle, registration_api, token)
        .map_err(|error| error.into_hal_error("prepared callback artifact commit failed"))
''',
)
replace_once(
    obj,
    '''fn callback_artifact_registration_runtime_lock_failure_error(
    context: &SharedAidlServiceContext,
    command: OwnerCallbackCleanupArtifactCommand,
    artifact_result: &Result<PreparedCallbackArtifactToken, HalError>,
    runtime_error: HalError,
) -> HalError {
''',
    '''fn callback_artifact_registration_runtime_lock_failure_error(
    context: &SharedAidlServiceContext,
    command: OwnerCallbackCleanupArtifactCommand,
    artifact_result: Result<PreparedCallbackArtifactToken, HalError>,
    runtime_error: HalError,
) -> HalError {
''',
)
replace_once(
    obj,
    '''    if let Ok(token) = artifact_result {
        if let Err(cleanup_error) =
            abort_prepared_callback_artifact_bridge(context, &command, *token)
''',
    '''    if let Ok(token) = artifact_result {
        if let Err(cleanup_error) =
            abort_prepared_callback_artifact_bridge(context, &command, token)
''',
)
replace_once(
    obj,
    '''                        &artifact_retain_result,
                        runtime_error,
''',
    '''                        artifact_retain_result,
                        runtime_error,
''',
)

child = "tuner_hal2/aidl_service/src/child_object_open.rs"
replace_once(
    child,
    "use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};\n",
    "use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};\n",
)
replace_once(
    child,
    '''        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenFilter, artifact);
            return finish_filter_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                filter_id,
                HalError::internal(HalInternalKind::InvariantViolation, "filter object kind mismatch"),
            );
        }
''',
    '''        Err(_) => {
            let primary = HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter object kind mismatch",
            );
            let primary = match context.abort_child_callback_artifact(
                child_handle,
                AidlApi::DemuxOpenFilter,
                artifact,
            ) {
                Ok(()) => primary,
                Err(cleanup) => compose_primary_cleanup_failure(
                    "prepared filter callback abort failed after Binder object construction failure",
                    primary,
                    cleanup,
                ),
            };
            return finish_filter_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                filter_id,
                primary,
            );
        }
''',
)
replace_once(
    child,
    '''        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenDvr, artifact);
            return finish_dvr_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                dvr_id,
                HalError::internal(HalInternalKind::InvariantViolation, "DVR object kind mismatch"),
            );
        }
''',
    '''        Err(_) => {
            let primary = HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR object kind mismatch",
            );
            let primary = match context.abort_child_callback_artifact(
                child_handle,
                AidlApi::DemuxOpenDvr,
                artifact,
            ) {
                Ok(()) => primary,
                Err(cleanup) => compose_primary_cleanup_failure(
                    "prepared DVR callback abort failed after Binder object construction failure",
                    primary,
                    cleanup,
                ),
            };
            return finish_dvr_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                dvr_id,
                primary,
            );
        }
''',
)

# Static invariants for the review findings.
callback_text = Path(store).read_text()
if "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken" in callback_text:
    raise SystemExit("PreparedCallbackArtifactToken is still Clone/Copy")
if "(PreparedCallbackArtifactToken, StoredCallback)" in callback_text:
    raise SystemExit("callback store still retains the authority wrapper")
service_text = Path(context).read_text()
if "let _ = self.callbacks" in service_text or "self.callbacks\n" in service_text:
    raise SystemExit("child callback bridge still uses/discards a shadow callback store path")
child_text = Path(child).read_text()
if "let _ = context.abort_child_callback_artifact" in child_text:
    raise SystemExit("child object failure still discards callback abort failure")
