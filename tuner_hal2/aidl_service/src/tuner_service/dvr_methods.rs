use super::support::local_filter_handle_from_strong;
use super::{
    build_dvr_configure_request, close_object_after_close_preflight, execute_object_query_use_case,
    execute_object_runtime_use_case, execute_object_runtime_use_case_with_request_builder,
    status_from_hal_error, tuner_queue_desc_from_snapshot, AidlMethodCall, AidlObjectGeneration,
    AidlObjectId, BinderResult, DvrAidlObject, DvrFilterLinkRequest, DvrSettings, IDvr, IFilter,
    Strong, TunerQueueDesc,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};

impl IDvr for DvrAidlObject {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        *queue = execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrGetQueueDesc,
            |runtime, handle| {
                runtime
                    .dvr_queue_descriptor_snapshot_for_aidl_object(
                        handle.object_id(),
                        handle.generation(),
                    )
                    .map(tuner_queue_desc_from_snapshot)
            },
        )?;
        Ok(())
    }
    fn configure(&self, settings: &DvrSettings) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request =
                    build_dvr_configure_request(settings).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::DvrConfigure(request.clone()), request))
            },
            |runtime, handle, command_plan, executable_request, _dispatch_preflight, request| {
                runtime.configure_dvr_runtime_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn attachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let filter_handle = local_filter_handle_from_strong(filter)?;
                let request = DvrFilterLinkRequest {
                    filter_id: filter_handle.object_id().0,
                    filter_generation: filter_handle.generation().0,
                };
                Ok((AidlMethodCall::DvrAttachFilter(request), request))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.attach_dvr_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    AidlObjectId(request.filter_id),
                    AidlObjectGeneration(request.filter_generation),
                    dispatch_preflight,
                )
            },
        )
    }
    fn detachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let filter_handle = local_filter_handle_from_strong(filter)?;
                let request = DvrFilterLinkRequest {
                    filter_id: filter_handle.object_id().0,
                    filter_generation: filter_handle.generation().0,
                };
                Ok((AidlMethodCall::DvrDetachFilter(request), request))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.detach_dvr_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    AidlObjectId(request.filter_id),
                    AidlObjectGeneration(request.filter_generation),
                    dispatch_preflight,
                )
            },
        )
    }
    fn start(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrStart,
            |runtime, handle, command_plan, executable_request| {
                runtime.start_dvr_for_object(
                    handle.object_id(),
                    handle.generation(),
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn stop(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrStop,
            |runtime, handle, command_plan, executable_request| {
                runtime.stop_dvr_for_object(
                    handle.object_id(),
                    handle.generation(),
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn flush(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrFlush,
            |runtime, handle, command_plan, executable_request| {
                runtime.flush_dvr_for_object(
                    handle.object_id(),
                    handle.generation(),
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(&self.runtime(), self.handle(), AidlMethodCall::DvrClose)
    }
    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let interval_ms = u64::try_from(milliseconds).map_err(|_| {
                    status_from_hal_error(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "DVR status check interval must be non-negative",
                    ))
                })?;
                Ok((
                    AidlMethodCall::DvrSetStatusCheckIntervalHint(milliseconds),
                    interval_ms,
                ))
            },
            |runtime,
             handle,
             _command_plan,
             _executable_request,
             dispatch_preflight,
             interval_ms| {
                runtime.set_dvr_status_check_interval_for_object(
                    handle.object_id(),
                    handle.generation(),
                    interval_ms,
                    dispatch_preflight,
                )
            },
        )
    }
}
