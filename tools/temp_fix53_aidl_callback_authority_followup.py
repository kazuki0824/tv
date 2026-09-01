from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1))


store = "tuner_hal2/aidl_service/src/callback_store.rs"
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
        self.commit_prepared_callback(handle, api, token)
            .map_err(|error| error.into_hal_error("prepared child callback commit failed"))
    }

    pub(crate) fn abort_child_callback_artifact(
        &self,
        handle: AidlObjectHandle,
        api: AidlApi,
        token: crate::callback_store::PreparedCallbackArtifactToken,
    ) -> Result<(), HalError> {
        self.abort_prepared_callback(handle, api, token)
            .map_err(|error| error.into_hal_error("prepared child callback abort failed"))
    }
''',
)
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
replace_once(
    obj,
    '''            let prepared_token = artifact_retain_result.as_ref().ok().copied();
            let outcome =
                guard.execute_callback_registration_after_artifact_result_for_object_use_case(
                    handle.object_kind(),
                    handle.object_id(),
                    handle.generation(),
                    api,
                    artifact_retain_result.map(|_| ()),
                    token,
                );
''',
    '''            let (artifact_result_for_runtime, prepared_token) = match artifact_retain_result {
                Ok(prepared_token) => (Ok(()), Some(prepared_token)),
                Err(error) => (Err(error), None),
            };
            let outcome =
                guard.execute_callback_registration_after_artifact_result_for_object_use_case(
                    handle.object_kind(),
                    handle.object_id(),
                    handle.generation(),
                    api,
                    artifact_result_for_runtime,
                    token,
                );
''',
)
replace_once(
    obj,
    '''                if !callback_store.commit_prepared_callback(
                    artifact_handle,
                    registration_api,
                    prepared_token,
                ) {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared callback artifact disappeared during composite commit",
                    ));
                }
''',
    '''                callback_store
                    .commit_prepared_callback(artifact_handle, registration_api, prepared_token)
                    .map_err(|error| {
                        error.into_hal_error(
                            "prepared callback artifact commit failed during composite commit",
                        )
                    })?;
''',
)
replace_once(
    obj,
    '''                Ok(if callback_store.abort_prepared_callback(
                    artifact_handle,
                    registration_api,
                    prepared_token,
                ) {
                    CallbackArtifactCleanupResult::Cleared
                } else {
                    CallbackArtifactCleanupResult::NoArtifact
                })
''',
    '''                callback_store
                    .abort_prepared_callback(artifact_handle, registration_api, prepared_token)
                    .map(|()| CallbackArtifactCleanupResult::Cleared)
                    .map_err(|error| {
                        error.into_hal_error(
                            "prepared callback artifact abort failed during registration rollback",
                        )
                    })
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
# Callback cleanup after Live publication failure is already owned by
# finish_*_child_object_construction_failure -> owner callback cleanup use-case.
replace_once(child, "        let _ = context.clear_owner_callbacks(child_handle);\n", "")
replace_once(child, "        let _ = context.clear_owner_callbacks(child_handle);\n", "")

contract = "tuner_hal2/compile_contract/src/lib.rs"
p = Path(contract)
text = p.read_text()
append = r'''

#[test]
fn aidl_callback_prepared_authority_source_contract() {
    const CALLBACK_STORE: &str = include_str!("../../aidl_service/src/callback_store.rs");
    const SERVICE_CONTEXT: &str = include_str!("../../aidl_service/src/service_context.rs");
    const OBJECT_RUNTIME: &str = include_str!("../../aidl_service/src/object_runtime/mod.rs");
    const CHILD_OPEN: &str = include_str!("../../aidl_service/src/child_object_open.rs");

    assert!(CALLBACK_STORE.contains("pub(crate) struct PreparedCallbackArtifactToken(u64);"));
    assert!(!CALLBACK_STORE.contains(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken"
    ));
    assert!(CALLBACK_STORE.contains("PreparedArtifactAuthorityMismatch"));
    assert!(!CALLBACK_STORE.contains("assert!(store.commit_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(store.abort_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(!store.commit_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(!store.abort_prepared_callback("));

    assert!(!SERVICE_CONTEXT.contains("-> Result<bool, AidlCallbackStoreError>"));
    assert!(!SERVICE_CONTEXT.contains("self.callbacks"));
    assert!(!SERVICE_CONTEXT.contains("let _ ="));

    assert!(!OBJECT_RUNTIME.contains(".ok().copied()"));
    assert!(!OBJECT_RUNTIME.contains("abort_prepared_callback_artifact_bridge(context, &command, *token)"));
    assert!(!OBJECT_RUNTIME.contains("if !callback_store.commit_prepared_callback("));
    assert!(!OBJECT_RUNTIME.contains("Ok(if callback_store.abort_prepared_callback("));

    assert!(!CHILD_OPEN.contains("let _ = context.abort_child_callback_artifact"));
    assert!(!CHILD_OPEN.contains("let _ = context.clear_owner_callbacks"));
    assert!(CHILD_OPEN.contains("compose_primary_cleanup_failure"));
}
'''
if "fn aidl_callback_prepared_authority_source_contract()" in text:
    raise SystemExit("compile contract already present")
p.write_text(text.rstrip() + append)

bp = "tuner_hal2/Android.bp"
replace_once(
    bp,
    '''    crate_root: "compile_contract/src/lib.rs",
    srcs: ["compile_contract/src/lib.rs"],
    edition: "2021",
''',
    '''    crate_root: "compile_contract/src/lib.rs",
    srcs: [
        "compile_contract/src/lib.rs",
        "aidl_service/src/callback_store.rs",
        "aidl_service/src/service_context.rs",
        "aidl_service/src/object_runtime/mod.rs",
        "aidl_service/src/child_object_open.rs",
    ],
    edition: "2021",
''',
)

workflow = ".github/workflows/tuner-hal2-host-rust-ci.yml"
p = Path(workflow)
text = p.read_text()
needle = '      - "tuner_hal2/compile_contract/**"\n'
if text.count(needle) != 2:
    raise SystemExit(f"host CI compile_contract path anchor count was {text.count(needle)}")
text = text.replace(
    needle,
    needle + '      - "tuner_hal2/aidl_service/**"\n      - "tuner_hal2/Android.bp"\n',
)
p.write_text(text)

# Fail before validation if any reviewed stale shape remains.
checks = {
    store: [
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken",
        "assert!(store.commit_prepared_callback(",
        "assert!(store.abort_prepared_callback(",
        "assert!(!store.commit_prepared_callback(",
        "assert!(!store.abort_prepared_callback(",
    ],
    context: ["self.callbacks", "-> Result<bool, AidlCallbackStoreError>"],
    obj: [
        ".ok().copied()",
        "abort_prepared_callback_artifact_bridge(context, &command, *token)",
        "if !callback_store.commit_prepared_callback(",
        "Ok(if callback_store.abort_prepared_callback(",
    ],
    child: [
        "let _ = context.abort_child_callback_artifact",
        "let _ = context.clear_owner_callbacks",
    ],
}
for path, stale_patterns in checks.items():
    current = Path(path).read_text()
    for pattern in stale_patterns:
        if pattern in current:
            raise SystemExit(f"{path}: stale reviewed pattern remains: {pattern!r}")
