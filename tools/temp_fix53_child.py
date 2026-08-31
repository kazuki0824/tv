from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    if s.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, got {s.count(old)}")
    p.write_text(s.replace(old, new, 1))

# Prepared object-table state is not a public/live object. ChildOpenTxn prepares
# it; AIDL constructs the callback artifact and Binder object; only then is it
# committed Live.
replace_once(
    "tuner_hal2/service_runtime/src/object_table.rs",
    """pub enum RuntimeObjectLifecycle {
    Live,
    Closing { step: CleanupStep },
""",
    """pub enum RuntimeObjectLifecycle {
    Prepared,
    Live,
    Closing { step: CleanupStep },
""",
)
replace_once(
    "tuner_hal2/service_runtime/src/object_table.rs",
    """    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Quarantined)
    }
""",
    """    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Quarantined)
    }
""",
)
replace_once(
    "tuner_hal2/service_runtime/src/object_table.rs",
    """    pub fn insert(&mut self, mut entry: RuntimeObjectEntry) -> Result<(), RuntimeObjectTableError> {
""",
    """    fn insert_with_lifecycle(
        &mut self,
        mut entry: RuntimeObjectEntry,
        lifecycle: RuntimeObjectLifecycle,
    ) -> Result<(), RuntimeObjectTableError> {
""",
)
replace_once(
    "tuner_hal2/service_runtime/src/object_table.rs",
    """        entry.lifecycle = RuntimeObjectLifecycle::Live;
        self.entries.insert(entry.object_id, entry);
        Ok(())
    }

    pub fn remove(
""",
    """        entry.lifecycle = lifecycle;
        self.entries.insert(entry.object_id, entry);
        Ok(())
    }

    pub fn insert(&mut self, entry: RuntimeObjectEntry) -> Result<(), RuntimeObjectTableError> {
        self.insert_with_lifecycle(entry, RuntimeObjectLifecycle::Live)
    }

    pub(crate) fn insert_prepared(
        &mut self,
        entry: RuntimeObjectEntry,
    ) -> Result<(), RuntimeObjectTableError> {
        self.insert_with_lifecycle(entry, RuntimeObjectLifecycle::Prepared)
    }

    pub(crate) fn commit_prepared(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self.entry_mut_checked_any(object_id, generation)?;
        if entry.lifecycle != RuntimeObjectLifecycle::Prepared {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle: entry.lifecycle,
            });
        }
        entry.lifecycle = RuntimeObjectLifecycle::Live;
        Ok(entry.clone())
    }

    pub fn remove(
""",
)
# Prepared is explicitly not a valid close/method state.
p = Path("tuner_hal2/service_runtime/src/object_table.rs")
s = p.read_text()
s = s.replace(
    "            RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupPending { .. } => {}\n",
    "            RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupPending { .. } => {}\n",
)
# Exhaustive matches: Prepared falls into existing lifecycle error arms where present.
p.write_text(s)

# Add prepared registration/commit helpers beside the existing runtime registration helper.
p = Path("tuner_hal2/service_runtime/src/boot.rs")
s = p.read_text()
anchor = "    pub(crate) fn record_child_open_rollback_diagnostic("
pos = s.index(anchor)
helpers = r'''
    pub(crate) fn register_prepared_aidl_object_for_runtime_auto_generation(
        &mut self,
        object_kind: AidlObjectKind,
        runtime_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        let generation = self.object_table.next_generation()?;
        let object_id = self.object_table.next_object_id()?;
        let entry = RuntimeObjectEntry {
            object_kind,
            object_id,
            generation,
            ledger_id: LedgerId(runtime_id),
            ledger_generation: LedgerGeneration(generation.0),
            owner,
            lifecycle: RuntimeObjectLifecycle::Prepared,
        };
        self.object_table.insert_prepared(entry.clone())?;
        Ok(entry)
    }

    pub fn commit_prepared_child_object(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, HalError> {
        self.object_table
            .commit_prepared(object_id, generation)
            .map_err(object_table_error_to_hal)
    }

'''
s = s[:pos] + helpers + s[pos:]
p.write_text(s)

# ChildOpenTxn now creates Prepared object-table entries instead of Live entries.
p = Path("tuner_hal2/service_runtime/src/boot/child_open_context.rs")
s = p.read_text().replace(
    ".register_aidl_object_for_runtime_auto_generation(\n",
    ".register_prepared_aidl_object_for_runtime_auto_generation(\n",
)
# exactly filter+dvr in this file
if s.count("register_prepared_aidl_object_for_runtime_auto_generation") < 2:
    raise SystemExit("child-open prepared registration replacement did not hit filter and DVR")
p.write_text(s)

# Callback store: filter/DVR use the same already-existing prepared artifact mechanism.
p = Path("tuner_hal2/aidl_service/src/callback_store.rs")
s = p.read_text()
insert_after = """    pub(crate) fn prepare_lnb_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn ILnbCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::LnbSetCallback);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks.insert(
            key,
            (token, StoredCallback::Lnb(callback.clone())),
        );
        Ok(token)
    }
"""
addition = insert_after + r'''

    pub(crate) fn prepare_filter_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFilterCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks.insert(
            key,
            (token, StoredCallback::Filter(callback.clone())),
        );
        Ok(token)
    }

    pub(crate) fn prepare_dvr_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IDvrCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks.insert(
            key,
            (token, StoredCallback::Dvr(callback.clone())),
        );
        Ok(token)
    }
'''
if s.count(insert_after) != 1:
    raise SystemExit("callback prepared insertion anchor mismatch")
s = s.replace(insert_after, addition, 1)
p.write_text(s)

# Service context exposes child callback preparation/commit/abort while keeping
# callback-store mutation behind its mutex.
p = Path("tuner_hal2/aidl_service/src/service_context.rs")
s = p.read_text()
anchor = "    pub(crate) fn runtime(&self) -> SharedTunerRuntime {\n"
pos = s.index(anchor)
helpers = r'''
    pub(crate) fn prepare_filter_callback_artifact(
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

'''
s = s[:pos] + helpers + s[pos:]
p.write_text(s)

# Child-open AIDL facade: prepare callback + construct Binder object while runtime
# entry is Prepared, then commit callback and Live immediately before returning.
p = Path("tuner_hal2/aidl_service/src/child_object_open.rs")
s = p.read_text()
start = s.index("fn finish_filter_child_open(\n")
mid = s.index("fn finish_dvr_child_open(\n", start)
end = len(s)
# end of dvr function is file end in current source.
new_filter = r'''fn finish_filter_child_open(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    runtime_open: maleicacid_tuner_hal2_service_runtime::FilterChildRuntimeOpen,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>> {
    let child_handle = handle_from_runtime_entry(runtime_open.runtime_entry);
    let filter_id = runtime_open.filter_id;
    let artifact = match context.prepare_filter_callback_artifact(child_handle, callback) {
        Ok(token) => token,
        Err(primary_error) => {
            return finish_filter_child_open_artifact_retain_failure(
                runtime, child_handle, filter_id, primary_error,
            )
        }
    };
    let object = match FilterAidlObject::new(child_handle, context.clone()) {
        Ok(object) => BnFilter::new_binder(object, BinderFeatures::default()),
        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenFilter, artifact);
            return finish_filter_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                filter_id,
                HalError::internal(HalInternalKind::InvariantViolation, "filter object kind mismatch"),
            );
        }
    };
    if let Err(primary) = context.commit_child_callback_artifact(
        child_handle,
        AidlApi::DemuxOpenFilter,
        artifact,
    ) {
        return finish_filter_child_open_artifact_retain_failure(runtime, child_handle, filter_id, primary);
    }
    if let Err(primary) = runtime
        .lock()
        .map_err(|_| status_from_hal_error(HalError::internal(HalInternalKind::InvariantViolation, "service runtime lock poisoned")))?
        .commit_prepared_child_object(child_handle.object_id(), child_handle.generation())
    {
        let _ = context.clear_owner_callbacks(child_handle);
        return finish_filter_child_object_construction_failure(
            context, runtime, child_handle, filter_id, primary,
        );
    }
    Ok(object)
}

'''
new_dvr = r'''fn finish_dvr_child_open(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    runtime_open: maleicacid_tuner_hal2_service_runtime::DvrChildRuntimeOpen,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>> {
    let child_handle = handle_from_runtime_entry(runtime_open.runtime_entry);
    let dvr_id = runtime_open.dvr_id;
    let artifact = match context.prepare_dvr_callback_artifact(child_handle, callback) {
        Ok(token) => token,
        Err(primary_error) => {
            return finish_dvr_child_open_artifact_retain_failure(runtime, child_handle, dvr_id, primary_error)
        }
    };
    let object = match DvrAidlObject::new(child_handle, context.clone()) {
        Ok(object) => BnDvr::new_binder(object, BinderFeatures::default()),
        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenDvr, artifact);
            return finish_dvr_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                dvr_id,
                HalError::internal(HalInternalKind::InvariantViolation, "DVR object kind mismatch"),
            );
        }
    };
    if let Err(primary) = context.commit_child_callback_artifact(
        child_handle,
        AidlApi::DemuxOpenDvr,
        artifact,
    ) {
        return finish_dvr_child_open_artifact_retain_failure(runtime, child_handle, dvr_id, primary);
    }
    if let Err(primary) = runtime
        .lock()
        .map_err(|_| status_from_hal_error(HalError::internal(HalInternalKind::InvariantViolation, "service runtime lock poisoned")))?
        .commit_prepared_child_object(child_handle.object_id(), child_handle.generation())
    {
        let _ = context.clear_owner_callbacks(child_handle);
        return finish_dvr_child_object_construction_failure(context, runtime, child_handle, dvr_id, primary);
    }
    Ok(object)
}
'''
s = s[:start] + new_filter + new_dvr + "\n"
p.write_text(s)
