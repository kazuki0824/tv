use super::{
    AidlMethodCall, AidlObjectKind, AvStreamType, BinderResult, DemuxFilterSettings,
    FilterAidlObject, FilterDelayHint, FilterReleaseAvHandleRequest,
    FilterSetDataSourceRequest, IFilter, RuntimeOwnerRelation, Strong,
    TunerNativeHandle, TunerQueueDesc, TunerResult, build_filter_av_stream_type_request,
    build_filter_delay_hint_request, build_filter_summary_for_open_type,
    status_from_hal_error, status_unknown_error, service_error
};
use super::support::{
    current_filter_open_type, demux_public_id_for_owner, filter_entry_public_id_and_owner,
    local_filter_handle_from_strong, runtime_entry_public_id,
    unavailable_after_method_plan
};

impl IFilter for FilterAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetQueueDesc),
            "FMQ runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::FilterClose)
    }
    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let open_type = current_filter_open_type(self)?;
        let config = build_filter_summary_for_open_type(settings, open_type)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterConfigure(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::ConfigureFilter(
                config.clone(),
            ),
        ))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .configure_filter_runtime_request(filter_id, config)
            .map_err(status_from_hal_error);
        result
    }
    fn configureAvStreamType(&self, av_stream_type: &AvStreamType) -> BinderResult<()> {
        self.ensure_open()?;
        let request =
            build_filter_av_stream_type_request(av_stream_type).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterConfigureAvStreamType(request))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .configure_filter_av_stream_type_request(filter_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn configureIpCid(&self, _ip_cid: i32) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterConfigure(maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::UnsupportedProfile { reason: "IP CID filtering is outside the TS-only tuner_hal2 profile" })),
            "IP CID filtering is outside the TS-only tuner_hal2 profile",
        )
    }
    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let plan = self.plan_method(AidlMethodCall::FilterConfigure(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::UnsupportedProfile {
                reason: "monitor event filtering is outside the TS-only tuner_hal2 profile",
            },
        ))?;
        if monitor_event_types == 0 {
            drop(plan);
            Ok(())
        } else {
            unavailable_after_method_plan(
                Ok(plan),
                "non-zero monitor event mask is outside the TS-only tuner_hal2 profile",
            )
        }
    }
    fn start(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterStart)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .start_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterStop)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .stop_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterFlush)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .flush_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn getAvSharedHandle(&self, _av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetAvSharedHandle),
            "AV shared memory is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSharedHandle unavailable path unexpectedly returned success",
        ))
    }
    fn getId(&self) -> BinderResult<i32> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterGetId)?;
        let runtime = self.runtime();
        runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)
    }
    fn getId64Bit(&self) -> BinderResult<i64> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterGetId64Bit)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        Ok(i64::from(filter_id))
    }
    fn releaseAvHandle(&self, _av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterReleaseAvHandle(
                FilterReleaseAvHandleRequest { av_data_id },
            )),
            "AV shared memory is not connected in current tuner_hal2 scope",
        )
    }
    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let runtime = self.runtime();
        let sink_handle = self.handle();
        let source_handle = local_filter_handle_from_strong(filter)?;
        if source_handle.object_id() == sink_handle.object_id()
            && source_handle.generation() == sink_handle.generation()
        {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "filter cannot use itself as source",
            ));
        }
        let (sink_id, sink_owner) = filter_entry_public_id_and_owner(&runtime, sink_handle)?;
        let (source_id, source_owner) = filter_entry_public_id_and_owner(&runtime, source_handle)?;
        if sink_owner != source_owner {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "source filter belongs to a different demux",
            ));
        }
        let RuntimeOwnerRelation::Demux { demux, generation } = sink_owner else {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "filter owner is not a demux",
            ));
        };
        let demux_id = demux_public_id_for_owner(&runtime, demux, generation)?;
        self.plan_method(AidlMethodCall::FilterSetDataSource(
            FilterSetDataSourceRequest {
                source_filter_id: source_handle.object_id().0,
                source_filter_generation: source_handle.generation().0,
            },
        ))?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_filter_data_source_non_null(demux_id, sink_id, source_id)
            .map_err(status_from_hal_error)?;
        Ok(())
    }
    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_filter_delay_hint_request(hint).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterSetDelayHint(request))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_filter_delay_hint_request(filter_id, request)
            .map_err(status_from_hal_error);
        result
    }
}
