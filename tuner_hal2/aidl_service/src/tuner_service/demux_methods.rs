use super::{
    AidlApi, AidlMethodCall, AidlObjectKind, BinderResult, DemuxFilterType, DvrType, IDemux, DemuxAidlObject,
    IDvr, IDvrCallback, IFilter, IFilterCallback, ITimeFilter, Strong,
    build_dvr_open_request, build_open_filter_request, open_dvr_child_after_plan,
    open_filter_child_after_plan, status_from_hal_error, status_unknown_error
};
use super::support::{
    runtime_entry_public_id, unavailable_after_object_public_api_plan,
    unsupported_public_api_call
};

impl IDemux for DemuxAidlObject {
    fn setFrontendDataSource(&self, frontend_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DemuxSetFrontendDataSource { frontend_id })?;
        let runtime = self.runtime();
        let demux_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_demux_frontend_data_source(demux_id, frontend_id)
            .map_err(status_from_hal_error)?;
        Ok(())
    }
    fn openFilter(
        &self,
        filter_type: &DemuxFilterType,
        buffer_size: i32,
        cb: &Strong<dyn IFilterCallback>,
    ) -> BinderResult<Strong<dyn IFilter>> {
        self.ensure_open()?;
        let open_request = build_open_filter_request(filter_type, buffer_size, true)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::DemuxOpenFilter(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::OpenFilter(
                open_request.clone(),
            ),
        ))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        open_filter_child_after_plan(&runtime, self.handle(), owner_demux_id, open_request, cb)
    }
    fn openTimeFilter(&self) -> BinderResult<Strong<dyn ITimeFilter>> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxOpenTimeFilter,
                None,
            )),
            "time filter is unsupported",
        )?;
        Err(status_unknown_error(
            "openTimeFilter unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncHwId(&self, _filter: &Strong<dyn IFilter>) -> BinderResult<i32> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxGetAvSyncHwId,
                None,
            )),
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncHwId unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncTime(&self, _av_sync_hw_id: i32) -> BinderResult<i64> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxGetAvSyncTime,
                None,
            )),
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncTime unavailable path unexpectedly returned success",
        ))
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DemuxClose)
    }
    fn openDvr(
        &self,
        dvr_type: DvrType,
        buffer_size: i32,
        cb: &Strong<dyn IDvrCallback>,
    ) -> BinderResult<Strong<dyn IDvr>> {
        self.ensure_open()?;
        let request =
            build_dvr_open_request(dvr_type, buffer_size).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::DemuxOpenDvr(request))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        open_dvr_child_after_plan(&runtime, self.handle(), owner_demux_id, request, cb)
    }
    fn connectCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxConnectCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
    fn disconnectCiCam(&self) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxDisconnectCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
}
