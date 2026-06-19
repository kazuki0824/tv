use super::support::unsupported_public_api_call;
use super::{
    build_dvr_open_request, build_open_filter_request, close_object_after_close_preflight,
    execute_object_runtime_use_case, open_dvr_child_for_owner_object_with_request_builder,
    open_filter_child_for_owner_object_with_request_builder,
    plan_unavailable_object_method_use_case, status_unknown_error, AidlApi, AidlMethodCall,
    AidlObjectKind, BinderResult, DemuxAidlObject, DemuxFilterType, DvrType, IDemux, IDvr,
    IDvrCallback, IFilter, IFilterCallback, ITimeFilter, Strong,
};

impl IDemux for DemuxAidlObject {
    fn setFrontendDataSource(&self, frontend_id: i32) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DemuxSetFrontendDataSource { frontend_id },
            |runtime, handle, command_plan, executable_request| {
                runtime.set_demux_frontend_data_source_for_object(
                    handle.object_id(),
                    handle.generation(),
                    frontend_id,
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn openFilter(
        &self,
        filter_type: &DemuxFilterType,
        buffer_size: i32,
        cb: &Strong<dyn IFilterCallback>,
    ) -> BinderResult<Strong<dyn IFilter>> {
        let runtime = self.runtime();
        open_filter_child_for_owner_object_with_request_builder(
            &runtime,
            self.handle(),
            || build_open_filter_request(filter_type, buffer_size, true),
            cb,
        )
    }
    fn openTimeFilter(&self) -> BinderResult<Strong<dyn ITimeFilter>> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Demux,
                    AidlApi::DemuxOpenTimeFilter,
                    None,
                ))
            },
            "time filter is unsupported",
        )?;
        Err(status_unknown_error(
            "openTimeFilter unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncHwId(&self, _filter: &Strong<dyn IFilter>) -> BinderResult<i32> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Demux,
                    AidlApi::DemuxGetAvSyncHwId,
                    None,
                ))
            },
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncHwId unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncTime(&self, _av_sync_hw_id: i32) -> BinderResult<i64> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Demux,
                    AidlApi::DemuxGetAvSyncTime,
                    None,
                ))
            },
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncTime unavailable path unexpectedly returned success",
        ))
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DemuxClose,
        )
    }
    fn openDvr(
        &self,
        dvr_type: DvrType,
        buffer_size: i32,
        cb: &Strong<dyn IDvrCallback>,
    ) -> BinderResult<Strong<dyn IDvr>> {
        let runtime = self.runtime();
        open_dvr_child_for_owner_object_with_request_builder(
            &runtime,
            self.handle(),
            || build_dvr_open_request(dvr_type, buffer_size),
            cb,
        )
    }
    fn connectCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Demux,
                    AidlApi::DemuxConnectCiCam,
                    None,
                ))
            },
            "CI CAM is unsupported",
        )
    }
    fn disconnectCiCam(&self) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Demux,
                    AidlApi::DemuxDisconnectCiCam,
                    None,
                ))
            },
            "CI CAM is unsupported",
        )
    }
}
