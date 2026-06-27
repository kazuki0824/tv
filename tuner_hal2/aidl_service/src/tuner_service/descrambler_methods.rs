use super::support::{local_filter_handle_from_strong, ts_pid_from_demux_pid};
use super::{
    close_object_after_close_preflight, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, status_from_hal_error, AidlMethodCall,
    BinderResult, DemuxPid, DescramblerAidlObject, IDescrambler, IFilter, Strong,
};

impl IDescrambler for DescramblerAidlObject {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DescramblerSetDemuxSource(demux_id),
            |runtime, handle, dispatch_proof| {
                runtime.set_descrambler_demux_source_for_object(
                    handle.object_id(),
                    handle.generation(),
                    demux_id,
                    dispatch_proof,
                )
            },
        )
    }

    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DescramblerSetKeyToken(key_token.to_vec()),
            |runtime, handle, dispatch_proof| {
                runtime.set_descrambler_key_token_for_object(
                    handle.object_id(),
                    handle.generation(),
                    key_token,
                    dispatch_proof,
                )
            },
        )
    }

    fn addPid(&self, pid: &DemuxPid, upstream_filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
                let source_handle = local_filter_handle_from_strong(upstream_filter)?;
                Ok((AidlMethodCall::DescramblerAddPid(pid), (pid, source_handle)))
            },
            |runtime, handle, dispatch_proof, (pid, source_handle)| {
                runtime.add_descrambler_pid_for_object(
                    handle.object_id(),
                    handle.generation(),
                    pid,
                    source_handle.object_id(),
                    source_handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }

    fn removePid(&self, pid: &DemuxPid, upstream_filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
                let source_handle = local_filter_handle_from_strong(upstream_filter)?;
                Ok((
                    AidlMethodCall::DescramblerRemovePid(pid),
                    (pid, source_handle),
                ))
            },
            |runtime, handle, dispatch_proof, (pid, source_handle)| {
                runtime.remove_descrambler_pid_for_object(
                    handle.object_id(),
                    handle.generation(),
                    pid,
                    source_handle.object_id(),
                    source_handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }

    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.context(),
            self.handle(),
            AidlMethodCall::DescramblerClose,
        )
    }
}
