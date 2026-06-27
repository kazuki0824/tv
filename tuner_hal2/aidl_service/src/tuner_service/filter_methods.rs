use super::support::{local_filter_handle_from_strong, public_api_call};
use super::{
    build_filter_av_stream_type_request, build_filter_delay_hint_request,
    build_filter_summary_for_open_type, close_object_after_close_preflight,
    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, plan_unavailable_object_method_use_case,
    status_from_hal_error, status_unknown_error, tuner_queue_desc_from_snapshot, AidlApi,
    AidlMethodCall, AidlObjectKind, AvStreamType, BinderResult, DemuxFilterSettings,
    FilterAidlObject, FilterDelayHint, FilterReleaseAvHandleRequest, FilterSetDataSourceRequest,
    IFilter, ObjectQueryRequest, ObjectQueryResponse, ParcelFileDescriptor, Strong,
    TunerNativeHandle, TunerQueueDesc,
};
use maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest;
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

impl IFilter for FilterAidlObject {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        *queue = match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FilterGetQueueDesc,
        )? {
            ObjectQueryResponse::QueueDescriptor(snapshot) => {
                tuner_queue_desc_from_snapshot(snapshot)
            }
            _ => {
                return Err(status_unknown_error(
                    "unexpected object query response for Filter.getQueueDesc",
                ))
            }
        };
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.context(),
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
            |runtime, handle, dispatch_proof| {
                runtime.configure_filter_runtime_for_object_with_current_open_type(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
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
            |runtime, handle, dispatch_proof, request| {
                runtime.configure_filter_av_stream_type_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
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
                |runtime, handle, dispatch_proof| {
                    runtime.plan_filter_runtime_noop_for_object(
                        handle.object_id(),
                        handle.generation(),
                        dispatch_proof,
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
            |runtime, handle, dispatch_proof| {
                runtime.start_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }

    fn stop(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterStop,
            |runtime, handle, dispatch_proof| {
                runtime.stop_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }

    fn flush(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterFlush,
            |runtime, handle, dispatch_proof| {
                runtime.flush_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }

    fn getAvSharedHandle(&self, av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        let export = execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterGetAvSharedHandle,
            |runtime, handle, dispatch_proof| {
                runtime.export_filter_av_shared_handle_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )?;
        let size_bytes = i64::try_from(export.size_bytes).map_err(|_| {
            status_from_hal_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "AV shared backing size does not fit i64",
            ))
        })?;
        *av_memory = TunerNativeHandle {
            fds: vec![ParcelFileDescriptor::new(export.file)],
            ints: Vec::new(),
        };
        Ok(size_bytes)
    }

    fn getId(&self) -> BinderResult<i32> {
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FilterGetId,
        )? {
            ObjectQueryResponse::PublicId(id) => Ok(id),
            _ => Err(status_unknown_error(
                "unexpected object query response for Filter.getId",
            )),
        }
    }

    fn getId64Bit(&self) -> BinderResult<i64> {
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FilterGetId64Bit,
        )? {
            ObjectQueryResponse::PublicId64(id) => Ok(id),
            _ => Err(status_unknown_error(
                "unexpected object query response for Filter.getId64Bit",
            )),
        }
    }

    fn releaseAvHandle(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FilterReleaseAvHandle(FilterReleaseAvHandleRequest { av_data_id }),
            |runtime, handle, dispatch_proof| {
                runtime.release_filter_av_handle_for_object(
                    handle.object_id(),
                    handle.generation(),
                    !av_memory.fds.is_empty(),
                    av_data_id,
                    dispatch_proof,
                )
            },
        )
    }

    fn setDataSource(&self, filter: Option<&Strong<dyn IFilter>>) -> BinderResult<()> {
        let sink_handle = self.handle();
        match filter {
            Some(filter) => execute_object_runtime_use_case_with_request_builder(
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
                |runtime, handle, dispatch_proof, source_handle| {
                    runtime
                        .set_filter_data_source_for_object(
                            handle.object_id(),
                            handle.generation(),
                            source_handle.object_id(),
                            source_handle.generation(),
                            dispatch_proof,
                        )
                        .map(|_| ())
                },
            ),
            None => execute_object_runtime_use_case(
                &self.runtime(),
                sink_handle,
                public_api_call(AidlObjectKind::Filter, AidlApi::FilterSetDataSource, None),
                |runtime, handle, dispatch_proof| {
                    runtime
                        .disconnect_filter_data_source_for_object(
                            handle.object_id(),
                            handle.generation(),
                            dispatch_proof,
                        )
                        .map(|_| ())
                },
            ),
        }
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
            |runtime, handle, dispatch_proof, request| {
                runtime.set_filter_delay_hint_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
                )
            },
        )
    }
}
