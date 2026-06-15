use super::{
    AidlMethodCall, AidlObjectKind, BinderResult, DemuxPid, DescramblerAidlObject,
    IDescrambler, IFilter, Strong, status_from_hal_error, status_unknown_error
};
use super::support::{
    filter_entry_public_id_and_owner, local_filter_handle_from_strong,
    runtime_entry_public_id, ts_pid_from_demux_pid
};

impl IDescrambler for DescramblerAidlObject {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DescramblerSetDemuxSource(demux_id))?;
        let runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_descrambler_demux_source(descrambler_id, demux_id)
            .map_err(status_from_hal_error);
        result
    }
    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DescramblerSetKeyToken(key_token.to_vec()))?;
        let runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_descrambler_key_token(descrambler_id, key_token)
            .map_err(status_from_hal_error);
        result
    }
    fn addPid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        let source_handle = local_filter_handle_from_strong(optional_upstream_filter)?;
        let self_runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&self_runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let (source_filter_id, _) = filter_entry_public_id_and_owner(&self_runtime, source_handle)?;
        self.plan_method(AidlMethodCall::DescramblerAddPid(pid))?;
        let result = self_runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .add_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn removePid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        let source_handle = local_filter_handle_from_strong(optional_upstream_filter)?;
        let self_runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&self_runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let (source_filter_id, _) = filter_entry_public_id_and_owner(&self_runtime, source_handle)?;
        self.plan_method(AidlMethodCall::DescramblerRemovePid(pid))?;
        let result = self_runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DescramblerClose)
    }
}
