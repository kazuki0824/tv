from pathlib import Path
import re

ROOT = Path("tuner_hal2")


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, got {count}: {old[:100]!r}")
    path.write_text(text.replace(old, new, 1))


# 10-25/59: site-local unsafe contracts for every libfmq native call.
fmq = ROOT / "fmq/src/lib.rs"
repls = [
    (
        '        let queue = unsafe { native_queue_create(num_bytes, configure_event_flag) };\n',
        '        // SAFETY: the shim accepts a byte capacity and flag by value and returns only an opaque\n'
        '        // queue pointer; no Rust reference crosses FFI and no caller-owned buffer is exposed.\n'
        '        let queue = unsafe { native_queue_create(num_bytes, configure_event_flag) };\n'
        '        // POSTCONDITION: a null pointer is treated as creation failure; every non-null pointer is\n'
        '        // uniquely owned by NativeFmqQueue until its single Drop destroys the native queue.\n',
    ),
    (
        '    pub(crate) fn available_to_read(&self) -> usize {\n        unsafe { native_queue_available_to_read(self.queue) }\n    }\n',
        '    pub(crate) fn available_to_read(&self) -> usize {\n'
        '        // SAFETY: self.queue is the non-null opaque handle uniquely created for this wrapper and\n'
        '        // remains live for the whole &self call; the shim receives no Rust-owned output pointer.\n'
        '        let available = unsafe { native_queue_available_to_read(self.queue) };\n'
        '        // POSTCONDITION: the returned count is a plain value; the call does not create a Rust alias\n'
        '        // to native storage and ownership of self.queue is unchanged.\n'
        '        available\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn available_to_write(&self) -> usize {\n        unsafe { native_queue_available_to_write(self.queue) }\n    }\n',
        '    pub(crate) fn available_to_write(&self) -> usize {\n'
        '        // SAFETY: self.queue is live and non-null for this shared borrow, and the shim returns only\n'
        '        // a byte count without exposing native storage through a Rust reference.\n'
        '        let available = unsafe { native_queue_available_to_write(self.queue) };\n'
        '        // POSTCONDITION: the result is detached scalar metadata and queue ownership/liveness remains\n'
        '        // with this NativeFmqQueue.\n'
        '        available\n'
        '    }\n',
    ),
    (
        '        let status = unsafe { native_queue_write_checked(self.queue, ptr, len, &mut written) };\n',
        '        // SAFETY: self.queue is live; ptr is null iff len==0, otherwise it points to data[0..len]\n'
        '        // for the duration of the call; &mut written is a valid unique out-parameter.\n'
        '        let status = unsafe { native_queue_write_checked(self.queue, ptr, len, &mut written) };\n'
        '        // POSTCONDITION: written is consumed only when the shim reports success; native code does not\n'
        '        // retain ptr/out_written, so the input slice and local out-parameter remain Rust-owned.\n',
    ),
    (
        '            unsafe { native_queue_read(self.queue, data.as_mut_ptr(), data.len()) }\n',
        '            // SAFETY: self.queue is live and data.as_mut_ptr() denotes a writable data.len()-byte\n'
        '            // region for the duration of the call; the non-empty branch excludes a dangling ZST use.\n'
        '            let read = unsafe { native_queue_read(self.queue, data.as_mut_ptr(), data.len()) };\n'
        '            // POSTCONDITION: the shim may initialize at most data.len() bytes and retains no pointer;\n'
        '            // the returned scalar is the only native description of how many bytes were produced.\n'
        '            read\n',
    ),
    (
        '        let status = unsafe { native_queue_read_exact(self.queue, data.as_mut_ptr(), data.len()) };\n',
        '        // SAFETY: self.queue is live and the non-empty mutable slice provides an exclusive writable\n'
        '        // region of exactly data.len() bytes; the shim does not retain the slice pointer.\n'
        '        let status = unsafe { native_queue_read_exact(self.queue, data.as_mut_ptr(), data.len()) };\n'
        '        // POSTCONDITION: status==0 means the complete requested region was initialized/consumed; on\n'
        '        // failure the caller treats the operation as failed and does not infer a successful exact read.\n',
    ),
    (
        '    pub(crate) fn wake(&self, bits: u32) -> i32 {\n        unsafe { native_queue_wake(self.queue, bits) }\n    }\n',
        '    pub(crate) fn wake(&self, bits: u32) -> i32 {\n'
        '        // SAFETY: self.queue is a live opaque queue handle and bits is passed by value; no Rust pointer\n'
        '        // other than the owned native handle crosses this call.\n'
        '        let status = unsafe { native_queue_wake(self.queue, bits) };\n'
        '        // POSTCONDITION: the integer status is returned verbatim for typed classification; queue\n'
        '        // ownership remains unchanged regardless of wake success or failure.\n'
        '        status\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn quantum(&self) -> i32 {\n        unsafe { native_queue_quantum(self.queue) }\n    }\n',
        '    pub(crate) fn quantum(&self) -> i32 {\n'
        '        // SAFETY: self.queue is live for the shared borrow and the shim only reads descriptor metadata.\n'
        '        let quantum = unsafe { native_queue_quantum(self.queue) };\n'
        '        // POSTCONDITION: quantum is detached scalar metadata; no native pointer escapes.\n'
        '        quantum\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn flags(&self) -> i32 {\n        unsafe { native_queue_flags(self.queue) }\n    }\n',
        '    pub(crate) fn flags(&self) -> i32 {\n'
        '        // SAFETY: self.queue is live for the shared borrow and the shim only reads descriptor flags.\n'
        '        let flags = unsafe { native_queue_flags(self.queue) };\n'
        '        // POSTCONDITION: flags is detached scalar metadata; queue ownership is unchanged.\n'
        '        flags\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn grantor_count(&self) -> usize {\n        unsafe { native_queue_grantor_count(self.queue) }\n    }\n',
        '    pub(crate) fn grantor_count(&self) -> usize {\n'
        '        // SAFETY: self.queue is live and the shim only reads the descriptor grantor count.\n'
        '        let count = unsafe { native_queue_grantor_count(self.queue) };\n'
        '        // POSTCONDITION: count is a scalar used to bound later grantor_at indices; no alias escapes.\n'
        '        count\n'
        '    }\n',
    ),
    (
        '        let ok = unsafe {\n            native_queue_grantor_at(self.queue, index, &mut fd_index, &mut offset, &mut extent)\n        };\n',
        '        // SAFETY: self.queue is live; index is treated as untrusted by the native accessor; all three\n'
        '        // out-parameters are valid unique pointers for this call and are not retained by the shim.\n'
        '        let ok = unsafe {\n            native_queue_grantor_at(self.queue, index, &mut fd_index, &mut offset, &mut extent)\n        };\n'
        '        // POSTCONDITION: the out-values are observed only when ok=true; a rejected/out-of-range index\n'
        '        // returns None so default locals are never interpreted as a descriptor grantor.\n',
    ),
    (
        '    pub(crate) fn fd_count(&self) -> usize {\n        unsafe { native_queue_fd_count(self.queue) }\n    }\n',
        '    pub(crate) fn fd_count(&self) -> usize {\n'
        '        // SAFETY: self.queue is live and the shim only reads the descriptor FD count.\n'
        '        let count = unsafe { native_queue_fd_count(self.queue) };\n'
        '        // POSTCONDITION: count is detached scalar metadata used to bound descriptor FD access.\n'
        '        count\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn dup_fd_at(&self, index: usize) -> i32 {\n        unsafe { native_queue_dup_fd_at(self.queue, index) }\n    }\n',
        '    pub(crate) fn dup_fd_at(&self, index: usize) -> i32 {\n'
        '        // SAFETY: self.queue is live and index is validated by the native accessor against the\n'
        '        // descriptor FD table; no Rust-owned FD is transferred into native code.\n'
        '        let fd = unsafe { native_queue_dup_fd_at(self.queue, index) };\n'
        '        // POSTCONDITION: a non-negative result is a newly duplicated caller-owned FD; a negative\n'
        '        // result is classified as failure and is never treated as an owned descriptor.\n'
        '        fd\n'
        '    }\n',
    ),
    (
        '    pub(crate) fn int_count(&self) -> usize {\n        unsafe { native_queue_int_count(self.queue) }\n    }\n',
        '    pub(crate) fn int_count(&self) -> usize {\n'
        '        // SAFETY: self.queue is live and the shim only reads the descriptor integer count.\n'
        '        let count = unsafe { native_queue_int_count(self.queue) };\n'
        '        // POSTCONDITION: count is detached scalar metadata used to bound integer descriptor access.\n'
        '        count\n'
        '    }\n',
    ),
    (
        '        let ok = unsafe { native_queue_int_at(self.queue, index, &mut value) };\n',
        '        // SAFETY: self.queue is live; index is checked by the native accessor and &mut value is a\n'
        '        // valid unique out-parameter which the shim does not retain.\n'
        '        let ok = unsafe { native_queue_int_at(self.queue, index, &mut value) };\n'
        '        // POSTCONDITION: value is observed only when ok=true; rejected indices become None.\n',
    ),
    (
        '    fn drop(&mut self) {\n        unsafe { native_queue_destroy(self.queue) };\n    }\n',
        '    fn drop(&mut self) {\n'
        '        // SAFETY: NativeFmqQueue uniquely owns this non-null handle and Drop runs once for that owner;\n'
        '        // no Rust reference to native internals exists and no later wrapper method can run afterward.\n'
        '        unsafe { native_queue_destroy(self.queue) };\n'
        '        // POSTCONDITION: the native queue is destroyed and self.queue must never be dereferenced or\n'
        '        // passed to the shim again; Rust is completing destruction of the sole wrapper owner.\n'
        '    }\n',
    ),
]
for old, new in repls:
    replace_once(fmq, old, new)

# 26-35/59: site-local dmabuf/FD/mmap/copy/munmap contracts.
backing = ROOT / "demux/src/av/shared_backing.rs"
backing_repls = [
    (
        '        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(size_bytes) };\n',
        '        // SAFETY: size_bytes is the checked backing-size result and is passed by value; the allocator\n'
        '        // returns a new FD or a negative error and receives no Rust pointer.\n'
        '        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(size_bytes) };\n'
        '        // POSTCONDITION: only a non-negative FD proceeds; ownership of that newly allocated descriptor\n'
        '        // is transferred exactly once into File below.\n',
    ),
    (
        '        let file = unsafe { File::from_raw_fd(raw_fd) };\n',
        '        // SAFETY: raw_fd was just returned non-negative by the allocator and has no other Rust owner;\n'
        '        // from_raw_fd therefore performs the unique ownership transfer into File.\n'
        '        let file = unsafe { File::from_raw_fd(raw_fd) };\n'
        '        // POSTCONDITION: file is now the sole Rust owner responsible for closing raw_fd; raw_fd must\n'
        '        // not be independently closed or wrapped again.\n',
    ),
    (
        '        let mapped = unsafe {\n            mmap(\n',
        '        // SAFETY: file owns a valid dmabuf FD and map_len is the checked backing extent; MAP_SHARED with\n'
        '        // read/write protection requests a mapping only for that FD/extent and no Rust reference exists yet.\n'
        '        let mapped = unsafe {\n            mmap(\n',
    ),
    (
        '        if mapped == libc::MAP_FAILED {\n',
        '        // POSTCONDITION: mapped is not dereferenced until MAP_FAILED is rejected; a successful pointer\n'
        '        // denotes exactly map_len writable bytes tracked by this function until the matching munmap.\n        if mapped == libc::MAP_FAILED {\n',
    ),
    (
        '        unsafe {\n            ptr::copy_nonoverlapping(payload.as_ptr(), destination, payload.len());\n        }\n',
        '        // SAFETY: candidate validation proves slot_offset + payload.len() <= map_len, mapped is a live\n'
        '        // writable mapping, payload is readable for payload.len(), and source/destination do not overlap.\n'
        '        unsafe {\n            ptr::copy_nonoverlapping(payload.as_ptr(), destination, payload.len());\n        }\n'
        '        // POSTCONDITION: exactly payload.len() bytes in the selected slot now contain the payload; the\n'
        '        // mapping and source slice remain separately owned and valid.\n',
    ),
    (
        '        let unmap_status = unsafe { munmap(mapped, map_len) };\n',
        '        // SAFETY: mapped is the successful live mapping returned above and map_len is the exact length\n'
        '        // used to create it; no Rust reference into the mapping is retained across this call.\n'
        '        let unmap_status = unsafe { munmap(mapped, map_len) };\n'
        '        // POSTCONDITION: after munmap returns the mapped range is treated as invalid regardless of status\n'
        '        // and is never dereferenced again; a nonzero status is surfaced as allocation failure.\n',
    ),
    (
        '        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(payload.len()) };\n',
        '        // SAFETY: payload.len() is passed by value and the native allocator receives no Rust pointer;\n'
        '        // it returns a fresh FD or a negative error code.\n'
        '        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(payload.len()) };\n'
        '        // POSTCONDITION: only a non-negative fresh FD proceeds and is transferred exactly once to File.\n',
    ),
    (
        '        let mapped = unsafe {\n            mmap(\n                ptr::null_mut(),\n                payload.len(),\n',
        '        // SAFETY: file uniquely owns the fresh dmabuf FD and payload is non-empty, so payload.len() is a\n'
        '        // valid requested mapping extent; no Rust reference points into the mapping before mmap succeeds.\n'
        '        let mapped = unsafe {\n            mmap(\n                ptr::null_mut(),\n                payload.len(),\n',
    ),
    (
        '        unsafe {\n            ptr::copy_nonoverlapping(payload.as_ptr(), mapped.cast::<u8>(), payload.len());\n        }\n',
        '        // SAFETY: mapped was checked against MAP_FAILED and denotes payload.len() writable bytes; payload\n'
        '        // is readable for the same length and the independent dmabuf mapping cannot overlap the slice.\n'
        '        unsafe {\n            ptr::copy_nonoverlapping(payload.as_ptr(), mapped.cast::<u8>(), payload.len());\n        }\n'
        '        // POSTCONDITION: the complete event payload now occupies the dmabuf mapping and no pointer is\n'
        '        // retained after the following unmap.\n',
    ),
    (
        '        let unmap_status = unsafe { munmap(mapped, payload.len()) };\n',
        '        // SAFETY: mapped is the successful event-local mapping and payload.len() is the exact mapping\n'
        '        // length; no Rust reference into the range survives this call.\n'
        '        let unmap_status = unsafe { munmap(mapped, payload.len()) };\n'
        '        // POSTCONDITION: the mapping is never accessed again after munmap; nonzero status is reported\n'
        '        // rather than treating uncertain mapping teardown as success.\n',
    ),
]
for old, new in backing_repls:
    replace_once(backing, old, new)

# S-06/S-07: prepared LNB lease and cleanup record are one-shot authorities.
registry = ROOT / "service_runtime/src/registry.rs"
replace_once(
    registry,
    '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct PreparedLnbAssignmentLease {\n',
    '#[derive(Debug, Eq, PartialEq)]\n#[must_use = "prepared LNB assignment lease must be committed or aborted by value"]\npub(crate) struct PreparedLnbAssignmentLease {\n',
)
replace_once(
    registry,
    '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct LnbAssignmentCleanupRecord {\n',
    '#[derive(Debug, Eq, PartialEq)]\n#[must_use = "LNB assignment cleanup authority must be completed by value"]\npub(crate) struct LnbAssignmentCleanupRecord {\n',
)

# S-08: clear-key preparation is not duplicable.
descr = ROOT / "service_runtime/src/descrambler_session.rs"
replace_once(
    descr,
    '#[derive(Clone, Debug, Eq, PartialEq)]\nstruct PreparedDescramblerClearKey {\n',
    '#[derive(Debug, Eq, PartialEq)]\n#[must_use = "prepared descrambler clear-key authority must be consumed by commit"]\nstruct PreparedDescramblerClearKey {\n',
)

# S-09: fixed-power preparation is a one-shot rollback authority.
lnb_ops = ROOT / "service_runtime/src/lnb_ops.rs"
replace_once(
    lnb_ops,
    '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) struct FrontendFixedPowerPreparation {\n',
    '#[derive(Debug, Eq, PartialEq)]\n#[must_use = "frontend fixed-power preparation must be completed or rolled back by value"]\npub(crate) struct FrontendFixedPowerPreparation {\n',
)
replace_once(
    lnb_ops,
    '    pub(crate) const fn frontend_id(self) -> FrontendRuntimeId {\n        self.frontend_id\n    }\n\n    pub(crate) const fn newly_retained(self) -> bool {\n        self.newly_retained\n    }\n',
    '    pub(crate) const fn frontend_id(&self) -> FrontendRuntimeId {\n        self.frontend_id\n    }\n\n    pub(crate) const fn newly_retained(&self) -> bool {\n        self.newly_retained\n    }\n',
)

# S-05: IDvr.start must not own the service-critical state transition.
dvr_methods = ROOT / "aidl_service/src/tuner_service/dvr_methods.rs"
replace_once(
    dvr_methods,
    '''        if playback_kind.is_err()\n            || matches!(playback_kind, Ok(true)) && worker_start_result.is_err()\n        {\n            let shared_runtime = self.runtime();\n            if let Ok(mut runtime) = shared_runtime.lock() {\n                runtime.mark_service_critical();\n            }\n        }\n''',
    '',
)

# Route the typed post-commit failure through a service-runtime owner use-case.
dvr_delivery = ROOT / "aidl_service/src/dvr_callback_delivery.rs"
replace_once(
    dvr_delivery,
    '''pub fn record_dvr_post_commit_notification_outcome(\n    context: &SharedAidlServiceContext,\n    handle: AidlObjectHandle,\n    phase: DvrPostCommitNotificationPhase,\n    outcome: Result<(), HalError>,\n) {\n    let Err(primary) = outcome else {\n        return;\n    };\n    record_dvr_callback_delivery_failure(\n        context,\n        handle,\n        CallbackDeliveryFailurePhase::PostCommitNotification,\n        phase,\n        primary,\n    );\n}\n''',
    '''pub fn record_dvr_post_commit_notification_outcome(\n    context: &SharedAidlServiceContext,\n    handle: AidlObjectHandle,\n    phase: DvrPostCommitNotificationPhase,\n    outcome: Result<(), HalError>,\n) {\n    let Err(primary) = outcome else {\n        return;\n    };\n    let finish_result = (|| -> Result<(), HalError> {\n        let runtime = context.runtime();\n        let mut guard = runtime.lock().map_err(|_| {\n            HalError::internal(\n                HalInternalKind::InvariantViolation,\n                "service runtime lock poisoned while finishing DVR post-commit notification failure",\n            )\n        })?;\n        guard.finish_dvr_post_commit_notification_failure_use_case(\n            handle.object_id(),\n            handle.generation(),\n            phase,\n            primary.clone(),\n        )\n    })();\n    if let Err(accounting_error) = finish_result {\n        record_post_commit_accounting_failure_fallback(\n            context,\n            handle,\n            phase,\n            DvrPostCommitNotificationFailureKind::CallbackRegistryAccounting,\n            primary,\n            "DVR post-commit notification failure accounting failed",\n            accounting_error,\n        );\n    }\n}\n''',
)

boot = ROOT / "service_runtime/src/boot.rs"
boot_text = boot.read_text()
anchor = '    pub fn finish_callback_delivery_failure_use_case(\n'
if boot_text.count(anchor) != 1:
    raise SystemExit("finish_callback_delivery_failure_use_case anchor missing or duplicated")
method = '''    pub fn finish_dvr_post_commit_notification_failure_use_case(\n        &mut self,\n        object_id: AidlObjectId,\n        generation: AidlObjectGeneration,\n        phase: DvrPostCommitNotificationPhase,\n        primary: HalError,\n    ) -> Result<(), HalError> {\n        let service_critical = if phase == DvrPostCommitNotificationPhase::StatusNotifierStart {\n            match self.dvr_status_metadata_snapshot_for_aidl_object(object_id, generation) {\n                Ok(snapshot) => snapshot.is_playback,\n                Err(_) => true,\n            }\n        } else {\n            false\n        };\n        self.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(\n            object_id,\n            generation,\n            CallbackDeliveryFailurePhase::PostCommitNotification,\n            phase,\n            primary,\n        ))?;\n        if service_critical {\n            self.mark_service_critical();\n        }\n        Ok(())\n    }\n\n'''
boot.write_text(boot_text.replace(anchor, method + anchor, 1))

# Static structural checks for this batch.
for path in (fmq, backing):
    lines = path.read_text().splitlines()
    for i, line in enumerate(lines):
        if 'unsafe {' not in line:
            continue
        before = '\n'.join(lines[max(0, i - 3):i])
        after = '\n'.join(lines[i + 1:min(len(lines), i + 8)])
        if '// SAFETY:' not in before:
            raise SystemExit(f"{path}:{i+1}: unsafe block lacks local SAFETY precondition")
        if '// POSTCONDITION:' not in after:
            raise SystemExit(f"{path}:{i+1}: unsafe block lacks local POSTCONDITION")

for name in [
    'PreparedLnbAssignmentLease', 'LnbAssignmentCleanupRecord',
    'PreparedDescramblerClearKey', 'FrontendFixedPowerPreparation',
]:
    hits = []
    for path in ROOT.rglob('*.rs'):
        text = path.read_text()
        if re.search(rf'^(?:pub(?:\([^\n)]*\))?\s+)?struct\s+{name}\b', text, re.M):
            hits.append(path)
    if len(hits) != 1:
        raise SystemExit(f"{name}: expected one declaration, got {hits}")
    text = hits[0].read_text()
    pos = re.search(rf'^(?:pub(?:\([^\n)]*\))?\s+)?struct\s+{name}\b', text, re.M).start()
    attrs = text[max(0, pos - 350):pos]
    if '#[must_use' not in attrs:
        raise SystemExit(f"{name}: must_use missing")
    derive_block = attrs.split('\n\n')[-1]
    if re.search(r'#\[derive\([^\]]*\b(?:Clone|Copy)\b', derive_block):
        raise SystemExit(f"{name}: duplicable derive remains")

if 'runtime.mark_service_critical()' in dvr_methods.read_text():
    raise SystemExit('IDvr method body still owns service-critical transition')
if 'finish_dvr_post_commit_notification_failure_use_case' not in dvr_delivery.read_text():
    raise SystemExit('typed DVR post-commit failure bridge missing')
if 'pub fn finish_dvr_post_commit_notification_failure_use_case' not in boot.read_text():
    raise SystemExit('service-runtime DVR post-commit failure owner missing')

print('review35 batch1 source contracts updated')
