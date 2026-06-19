use super::support::local_filter_handle_from_strong;
use super::{
    build_filter_av_stream_type_request, build_filter_delay_hint_request,
    build_filter_summary_for_open_type, close_object_after_close_preflight,
    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, plan_unavailable_object_method_use_case,
    status_from_hal_error, status_unknown_error, AidlMethodCall, AidlObjectKind, AvStreamType,
    BinderResult, DemuxFilterSettings, FilterAidlObject, FilterDelayHint,
    FilterReleaseAvHandleRequest, FilterSetDataSourceRequest, IFilter, Strong, TunerNativeHandle,
    TunerQueueDesc,
};
use maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest;

impl IFilter for FilterAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::FilterGetQueueDesc),
            "FMQ runtime is not connected in current tuner_hal2 scope",
        )
    }

    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterClose,
        )
    }

    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterConfigure(
                maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::ConfigureFilterByCurrentOpenType,
            ),
            |runtime, handle, command_plan, executable_request| {
                runtime.configure_filter_runtime_for_object_with_current_open_type(
                    handle.object_id(),
                    handle.generation(),
                    command_plan,
                    executable_request,
                    |open_type| build_filter_summary_for_open_type(settings, open_type),
                )
            },
        )
    }

    fn configureAvStreamType(&self, av_stream_type: &AvStreamType) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_filter_av_stream_type_request(av_stream_type)
                    .map_err(status_from_hal_error)?;
                Ok((
                    AidlMethodCall::FilterConfigureAvStreamType(request.clone()),
                    request,
                ))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.configure_filter_av_stream_type_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
            },
        )
    }

    fn configureIpCid(&self, _ip_cid: i32) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(AidlMethodCall::FilterConfigure(
                    RuntimeExecutableRequest::UnsupportedProfile {
                        reason: "IP CID filtering is outside the TS-only tuner_hal2 profile",
                    },
                ))
            },
            "IP CID filtering is outside the TS-only tuner_hal2 profile",
        )
    }

    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        if monitor_event_types == 0 {
            execute_object_runtime_use_case(
                &self.runtime(),
                self.handle(),
                AidlMethodCall::FilterConfigure(RuntimeExecutableRequest::NoPayload),
                |runtime, handle, command_plan, executable_request| {
                    runtime.plan_filter_runtime_noop_for_object(
                        handle.object_id(),
                        handle.generation(),
                        command_plan,
                        executable_request,
                    )
                },
            )
        } else {
            plan_unavailable_object_method_use_case(
                &self.runtime(),
                self.handle(),
                || {
                    Ok(AidlMethodCall::FilterConfigure(
                        RuntimeExecutableRequest::UnsupportedProfile {
                            reason:
                                "monitor event filtering is outside the TS-only tuner_hal2 profile",
                        },
                    ))
                },
                "non-zero monitor event mask is outside the TS-only tuner_hal2 profile",
            )
        }
    }

    fn start(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterStart,
            |runtime, handle, command_plan, executable_request| {
                runtime.start_filter_for_object(
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
            AidlMethodCall::FilterStop,
            |runtime, handle, command_plan, executable_request| {
                runtime.stop_filter_for_object(
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
            AidlMethodCall::FilterFlush,
            |runtime, handle, command_plan, executable_request| {
                runtime.flush_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    command_plan,
                    executable_request,
                )
            },
        )
    }

    fn getAvSharedHandle(&self, _av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::FilterGetAvSharedHandle),
            "AV shared memory is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSharedHandle unavailable path unexpectedly returned success",
        ))
    }

    fn getId(&self) -> BinderResult<i32> {
        execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterGetId,
            |runtime, handle| {
                runtime.public_runtime_id_for_object_method(
                    handle.object_id(),
                    handle.generation(),
                    AidlObjectKind::Filter,
                )
            },
        )
    }

    fn getId64Bit(&self) -> BinderResult<i64> {
        execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterGetId64Bit,
            |runtime, handle| {
                runtime
                    .public_runtime_id_for_object_method(
                        handle.object_id(),
                        handle.generation(),
                        AidlObjectKind::Filter,
                    )
                    .map(i64::from)
            },
        )
    }

    fn releaseAvHandle(&self, _av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(AidlMethodCall::FilterReleaseAvHandle(
                    FilterReleaseAvHandleRequest { av_data_id },
                ))
            },
            "AV shared memory is not connected in current tuner_hal2 scope",
        )
    }

    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        let sink_handle = self.handle();
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            sink_handle,
            || {
                let source_handle = local_filter_handle_from_strong(filter)?;
                Ok((
                    AidlMethodCall::FilterSetDataSource(FilterSetDataSourceRequest {
                        source_filter_id: source_handle.object_id().0,
                        source_filter_generation: source_handle.generation().0,
                    }),
                    source_handle,
                ))
            },
            |runtime,
             handle,
             _command_plan,
             _executable_request,
             dispatch_preflight,
             source_handle| {
                runtime
                    .set_filter_data_source_for_object(
                        handle.object_id(),
                        handle.generation(),
                        source_handle.object_id(),
                        source_handle.generation(),
                        dispatch_preflight,
                    )
                    .map(|_| ())
            },
        )
    }

    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request =
                    build_filter_delay_hint_request(hint).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::FilterSetDelayHint(request.clone()), request))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.set_filter_delay_hint_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
            },
        )
    }
}
