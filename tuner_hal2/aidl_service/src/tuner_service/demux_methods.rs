use super::support::{
    local_filter_handle_from_strong, public_api_call, unsupported_public_api_call,
};
use super::{
    build_dvr_open_request, build_open_filter_request, close_object_after_close_preflight,
    execute_object_query_use_case, execute_object_query_use_case_with_aidl_input_conversion,
    execute_object_runtime_use_case, open_dvr_child_for_owner_object_with_request_builder,
    open_filter_child_for_owner_object_with_request_builder,
    plan_unavailable_object_method_use_case, status_unknown_error, AidlApi, AidlMethodCall,
    AidlObjectKind, BinderResult, DemuxAidlObject, DemuxFilterType, DvrType, IDemux, IDvr,
    IDvrCallback, IFilter, IFilterCallback, ITimeFilter, ObjectQueryRequest, ObjectQueryResponse,
    Strong,
};

impl IDemux for DemuxAidlObject {
    fn setFrontendDataSource(&self, frontend_id: i32) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DemuxSetFrontendDataSource { frontend_id },
            |runtime, handle, dispatch_proof| {
                runtime.set_demux_frontend_data_source_for_object(
                    handle.object_id(),
                    handle.generation(),
                    frontend_id,
                    dispatch_proof,
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
        open_filter_child_for_owner_object_with_request_builder(
            &self.context(),
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
    fn getAvSyncHwId(&self, filter: &Strong<dyn IFilter>) -> BinderResult<i32> {
        match execute_object_query_use_case_with_aidl_input_conversion(
            &self.runtime(),
            self.handle(),
            public_api_call(AidlObjectKind::Demux, AidlApi::DemuxGetAvSyncHwId, None),
            || {
                let filter_handle = local_filter_handle_from_strong(filter)?;
                Ok(ObjectQueryRequest::DemuxGetAvSyncHwId {
                    filter_object_id: filter_handle.object_id(),
                    filter_generation: filter_handle.generation(),
                })
            },
        )? {
            ObjectQueryResponse::AvSyncHwId(id) => Ok(id),
            _ => Err(status_unknown_error(
                "unexpected object query response for Demux.getAvSyncHwId",
            )),
        }
    }
    fn getAvSyncTime(&self, av_sync_hw_id: i32) -> BinderResult<i64> {
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::DemuxGetAvSyncTime { av_sync_hw_id },
        )? {
            ObjectQueryResponse::AvSyncTime(time) => Ok(time),
            _ => Err(status_unknown_error(
                "unexpected object query response for Demux.getAvSyncTime",
            )),
        }
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.context(),
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
        open_dvr_child_for_owner_object_with_request_builder(
            &self.context(),
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
