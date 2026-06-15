use super::{
    AidlMethodCall, BinderResult, DvrAidlObject, DvrFilterLinkRequest, DvrSettings, IDvr,
    IFilter, Strong, TunerQueueDesc, build_dvr_configure_request, status_from_hal_error
};
use super::support::{
    local_filter_handle_from_strong, unavailable_after_method_plan
};

impl IDvr for DvrAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrGetQueueDesc),
            "DVR FMQ runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn configure(&self, settings: &DvrSettings) -> BinderResult<()> {
        let request = build_dvr_configure_request(settings).map_err(status_from_hal_error)?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrConfigure(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn attachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_handle = local_filter_handle_from_strong(filter)?;
        let request = DvrFilterLinkRequest {
            filter_id: filter_handle.object_id().0,
            filter_generation: filter_handle.generation().0
};
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrAttachFilter(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn detachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_handle = local_filter_handle_from_strong(filter)?;
        let request = DvrFilterLinkRequest {
            filter_id: filter_handle.object_id().0,
            filter_generation: filter_handle.generation().0
};
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrDetachFilter(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn start(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStart),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn stop(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStop),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn flush(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrFlush),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DvrClose)
    }
    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrSetStatusCheckIntervalHint(milliseconds)),
            "DVR callback runtime is not connected in current tuner_hal2 scope",
        )
    }
}
