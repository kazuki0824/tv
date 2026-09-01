from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} anchors, got {count}: {old[:180]!r}")
    p.write_text(text.replace(old, new))


store = "tuner_hal2/aidl_service/src/callback_store.rs"
replace_exact(
    store,
    '''    #[test]
    fn prepared_replacement_abort_preserves_current_callback() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        store.retain_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }
''',
    '''    #[test]
    fn prepared_replacement_abort_preserves_current_callback() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        store.retain_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert_eq!(
            store.abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }
''',
)
replace_exact(
    store,
    '''    #[test]
    fn prepared_replacement_becomes_current_only_at_commit() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(!store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.commit_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }
''',
    '''    #[test]
    fn prepared_replacement_becomes_current_only_at_commit() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(!store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert_eq!(
            store.commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }
''',
)
replace_exact(
    store,
    '''    #[test]
    fn stale_prepared_token_cannot_commit_or_abort_another_artifact() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let stale = PreparedCallbackArtifactToken(token.0 + 1);

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
    }
''',
    '''    #[test]
    fn stale_prepared_token_cannot_commit_or_abort_another_artifact() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let stale_id = token.0 + 1;

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
    }
''',
)
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
    obj,
    '''                        &artifact_retain_result,
                        runtime_error,
''',
    '''                        artifact_retain_result,
                        runtime_error,
''',
)
replace_exact(
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
replace_exact(
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
replace_exact(
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
replace_exact(
    child,
    "use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};\n",
    "use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};\n",
)
replace_exact(
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
replace_exact(
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
replace_exact(child, "        let _ = context.clear_owner_callbacks(child_handle);\n", "", expected=2)

contract = "tuner_hal2/compile_contract/src/lib.rs"
p = Path(contract)
text = p.read_text()
if "fn aidl_callback_prepared_authority_source_contract()" in text:
    raise SystemExit("AIDL callback authority source contract already exists")
text = text.rstrip() + r'''

#[test]
fn aidl_callback_prepared_authority_source_contract() {
    const CALLBACK_STORE: &str = include_str!("../../aidl_service/src/callback_store.rs");
    const SERVICE_CONTEXT: &str = include_str!("../../aidl_service/src/service_context.rs");
    const OBJECT_RUNTIME: &str = include_str!("../../aidl_service/src/object_runtime/mod.rs");
    const CHILD_OPEN: &str = include_str!("../../aidl_service/src/child_object_open.rs");

    fn function_slice<'a>(source: &'a str, name: &str) -> &'a str {
        let marker = format!("fn {name}(");
        let start = source.find(&marker).unwrap_or_else(|| panic!("missing {name}"));
        let tail = &source[start..];
        let end = tail[marker.len()..]
            .find("\n    pub(crate) fn ")
            .map(|offset| marker.len() + offset)
            .unwrap_or(tail.len());
        &tail[..end]
    }

    assert!(CALLBACK_STORE.contains("pub(crate) struct PreparedCallbackArtifactToken(u64);"));
    assert!(!CALLBACK_STORE.contains(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken"
    ));
    assert!(CALLBACK_STORE.contains("PreparedArtifactAuthorityMismatch"));
    assert!(!CALLBACK_STORE.contains("assert!(store.commit_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(store.abort_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(!store.commit_prepared_callback("));
    assert!(!CALLBACK_STORE.contains("assert!(!store.abort_prepared_callback("));

    let commit_bridge = function_slice(SERVICE_CONTEXT, "commit_prepared_callback");
    let abort_bridge = function_slice(SERVICE_CONTEXT, "abort_prepared_callback");
    assert!(commit_bridge.contains("Result<(), AidlCallbackStoreError>"));
    assert!(abort_bridge.contains("Result<(), AidlCallbackStoreError>"));
    assert!(!commit_bridge.contains("Result<bool"));
    assert!(!abort_bridge.contains("Result<bool"));
    assert!(!SERVICE_CONTEXT.contains("self.callbacks"));

    assert!(!OBJECT_RUNTIME.contains(".ok().copied()"));
    assert!(!OBJECT_RUNTIME.contains("abort_prepared_callback_artifact_bridge(context, &command, *token)"));
    assert!(!OBJECT_RUNTIME.contains("if !callback_store.commit_prepared_callback("));
    assert!(!OBJECT_RUNTIME.contains("Ok(if callback_store.abort_prepared_callback("));

    assert!(!CHILD_OPEN.contains("let _ = context.abort_child_callback_artifact"));
    assert!(!CHILD_OPEN.contains("let _ = context.clear_owner_callbacks"));
    assert!(CHILD_OPEN.contains("compose_primary_cleanup_failure"));
}
'''
p.write_text(text)

bp = "tuner_hal2/Android.bp"
replace_exact(
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
path_anchor = '      - "tuner_hal2/compile_contract/**"\n'
if text.count(path_anchor) != 2:
    raise SystemExit(f"host CI path anchor count was {text.count(path_anchor)}")
text = text.replace(
    path_anchor,
    path_anchor + '      - "tuner_hal2/aidl_service/**"\n      - "tuner_hal2/Android.bp"\n',
)
rustfmt_anchor = '''      - name: Check Rust formatting
        run: cargo fmt --all -- --check
'''
if text.count(rustfmt_anchor) != 1:
    raise SystemExit("host CI rustfmt anchor mismatch")
text = text.replace(
    rustfmt_anchor,
    rustfmt_anchor + '''
      - name: Check reviewed AIDL authority sources formatting
        run: |
          rustfmt --edition 2021 --check ../aidl_service/src/callback_store.rs
          rustfmt --edition 2021 --check ../aidl_service/src/service_context.rs
          rustfmt --edition 2021 --check ../aidl_service/src/object_runtime/mod.rs
          rustfmt --edition 2021 --check ../aidl_service/src/child_object_open.rs
''',
    1,
)
host_check_anchor = '''      - name: Type-check host-compatible crates
        run: cargo check --workspace --all-targets --locked
'''
if text.count(host_check_anchor) != 1:
    raise SystemExit("host CI check anchor mismatch")
text = text.replace(
    host_check_anchor,
    '''      - name: Run AIDL callback authority source contract
        run: cargo test -p maleicacid-tuner-hal2-compile-contract-host-ci --locked

''' + host_check_anchor,
    1,
)
p.write_text(text)

# Reviewed stale shapes must be absent after the repair.
checks = {
    store: [
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedCallbackArtifactToken",
        "assert!(store.commit_prepared_callback(",
        "assert!(store.abort_prepared_callback(",
        "assert!(!store.commit_prepared_callback(",
        "assert!(!store.abort_prepared_callback(",
    ],
    context: ["self.callbacks"],
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
for path, patterns in checks.items():
    current = Path(path).read_text()
    for pattern in patterns:
        if pattern in current:
            raise SystemExit(f"{path}: stale reviewed pattern remains: {pattern!r}")
