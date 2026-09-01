#[cfg(test)]
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;

#[test]
fn frontend_worker_stop_ticket_is_opaque_single_use_contract() {
    static_assertions::assert_not_impl_any!(FrontendWorkerStopTicket: Clone, Copy);
}

#[test]
fn aidl_callback_prepared_authority_source_contract() {
    const CALLBACK_STORE: &str = include_str!("../../aidl_service/src/callback_store.rs");
    const SERVICE_CONTEXT: &str = include_str!("../../aidl_service/src/service_context.rs");
    const OBJECT_RUNTIME: &str = include_str!("../../aidl_service/src/object_runtime/mod.rs");
    const CHILD_OPEN: &str = include_str!("../../aidl_service/src/child_object_open.rs");

    fn function_slice<'a>(source: &'a str, name: &str) -> &'a str {
        let marker = format!("fn {name}(");
        let start = source
            .find(&marker)
            .unwrap_or_else(|| panic!("missing {name}"));
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
    assert!(!OBJECT_RUNTIME
        .contains("abort_prepared_callback_artifact_bridge(context, &command, *token)"));
    assert!(!OBJECT_RUNTIME.contains("if !callback_store.commit_prepared_callback("));
    assert!(!OBJECT_RUNTIME.contains("Ok(if callback_store.abort_prepared_callback("));

    assert!(!CHILD_OPEN.contains("let _ = context.abort_child_callback_artifact"));
    assert!(!CHILD_OPEN.contains("let _ = context.clear_owner_callbacks"));
    assert!(CHILD_OPEN.contains("compose_primary_cleanup_failure"));
}

#[test]
fn playback_consume_error_conversion_source_contract() {
    const PLAYBACK_CONSUME: &str =
        include_str!("../../service_runtime/src/playback_consume_txn.rs");

    assert!(PLAYBACK_CONSUME.contains(
        "None => return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id).into()),"
    ));
    assert!(!PLAYBACK_CONSUME.contains(
        "None => return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id)),"
    ));
}
