from pathlib import Path
R=Path('tuner_hal2')
def one(p,o,n):
    t=p.read_text(); c=t.count(o)
    if c!=1: raise SystemExit(f'{p}: expected one anchor, got {c}: {o[:100]!r}')
    p.write_text(t.replace(o,n,1))

# S-10 remainder: AIDL method is now a pure descriptor adapter calling object_runtime façade.
fm=R/'aidl_service/src/tuner_service/filter_methods.rs'
one(fm,
'''    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, plan_unavailable_object_method_use_case,
''',
'''    execute_filter_av_handle_release_use_case, execute_object_query_use_case,
    execute_object_runtime_use_case, execute_object_runtime_use_case_with_request_builder,
    plan_unavailable_object_method_use_case,
''')
t=fm.read_text(); a=t.index('    fn releaseAvHandle('); b=t.index('    fn setDataSource(',a)
new='''    fn releaseAvHandle(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        execute_filter_av_handle_release_use_case(
            &self.runtime(),
            self.handle(),
            av_data_id,
            || match (av_memory.fds.as_slice(), av_memory.ints.as_slice()) {
                ([], []) => Ok(AvHandleReleaseDescriptor::Empty),
                ([file], [0]) => {
                    let metadata = std::fs::metadata(format!("/proc/self/fd/{}", file.as_raw_fd()))
                        .map_err(|_| HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "AV release handle identity could not be classified safely",
                        ))?;
                    Ok(AvHandleReleaseDescriptor::File(AvFileIdentity::new(
                        metadata.dev(), metadata.ino(), metadata.size(),
                    )))
                }
                _ => Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV handle shape is neither empty nor a single exported allocation handle",
                )),
            },
        )
    }

'''
fm.write_text(t[:a]+new+t[b:])

# S-12: fixed-power registry mutations are typed FrontendTxn entries.
ft=R/'service_runtime/src/boot/frontend_txn.rs'
t=ft.read_text(); anchor="impl<'a> FrontendTxn<'a> {\n"
if t.count(anchor)!=1: raise SystemExit('FrontendTxn impl anchor')
methods='''impl<'a> FrontendTxn<'a> {
    pub(crate) fn retain_fixed_power_lease(
        &mut self,
        frontend_id: crate::registry::FrontendRuntimeId,
        lnb_id: crate::registry::LnbRuntimeId,
    ) -> Result<bool, HalError> {
        self.runtime.registry.retain_frontend_fixed_power_lease(frontend_id, lnb_id)
    }

    pub(crate) fn release_fixed_power_lease(
        &mut self,
        frontend_id: crate::registry::FrontendRuntimeId,
    ) -> Result<Option<(crate::registry::LnbRuntimeId, usize)>, HalError> {
        self.runtime.registry.release_frontend_fixed_power_lease(frontend_id)
    }

    pub(crate) fn reopen_fixed_power_lnb(
        &mut self,
        lnb_id: crate::registry::LnbRuntimeId,
    ) -> Result<(), HalError> {
        self.runtime.registry.reopen_lnb(lnb_id).map_err(crate::boot::lnb_txn::map_lnb_failure)
    }

'''
ft.write_text(t.replace(anchor,methods,1))

lo=R/'service_runtime/src/lnb_ops.rs'
t=lo.read_text()
repls={
'.registry_mut()\n        .retain_frontend_fixed_power_lease(frontend_id, lnb_id)':'.frontend_txn()\n        .retain_fixed_power_lease(frontend_id, lnb_id)',
'.registry_mut()\n        .release_frontend_fixed_power_lease(frontend_id)':'.frontend_txn()\n        .release_fixed_power_lease(frontend_id)',
'.registry_mut()\n                .retain_frontend_fixed_power_lease(frontend_id, lnb_id)':'.frontend_txn()\n                .retain_fixed_power_lease(frontend_id, lnb_id)',
'.registry_mut()\n                    .reopen_lnb(lnb_id)\n                    .map_err(crate::boot::lnb_txn::map_lnb_failure)':'.frontend_txn()\n                    .reopen_fixed_power_lnb(lnb_id)',
'.registry_mut()\n                .release_frontend_fixed_power_lease(frontend_id)?':'.frontend_txn()\n                .release_fixed_power_lease(frontend_id)?',
}
for old,new in repls.items(): t=t.replace(old,new)
lo.write_text(t)

# Source contracts for S-03/S-04/S-10/S-12.
cleanup=(R/'aidl_service/src/cleanup_reaper.rs').read_text()
assert 'std::thread::Builder' not in cleanup
assert 'Condvar' not in cleanup
assert 'WorkerRuntimeReaperQueue' in cleanup
thread_owner=(R/'device/src/runtime/thread_result_owner.rs').read_text()
for bad in ['JoinHandle','Condvar','thread::Builder','catch_unwind','Arc<Mutex']:
    assert bad not in thread_owner, bad
assert 'WorkerRuntimeResultOwner' in thread_owner
body=fm.read_text(); body=body[body.index('fn releaseAvHandle'):body.index('fn setDataSource')]
assert 'preflight_filter_av_handle_release_for_any_lifecycle' not in body
assert '.lock()' not in body
assert 'execute_filter_av_handle_release_use_case' in body
lnb=lo.read_text()
assert 'registry_mut()\n        .retain_frontend_fixed_power_lease' not in lnb
assert 'registry_mut()\n        .release_frontend_fixed_power_lease' not in lnb
assert 'fn retain_fixed_power_lease' in ft.read_text()
assert 'fn release_fixed_power_lease' in ft.read_text()
print('review35 batch2 tail applied')
