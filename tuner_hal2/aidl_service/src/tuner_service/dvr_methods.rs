use super::support::local_filter_handle_from_strong;
use super::{
    build_dvr_configure_request, close_object_after_close_preflight, execute_object_query_use_case,
    plan_unavailable_object_method_use_case, status_from_hal_error, tuner_queue_desc_from_snapshot,
    AidlMethodCall, BinderResult, DvrAidlObject, DvrFilterLinkRequest, DvrSettings, IDvr, IFilter,
    Strong, TunerQueueDesc,
};

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
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                let request =
                    build_dvr_configure_request(settings).map_err(status_from_hal_error)?;
                Ok(AidlMethodCall::DvrConfigure(request))
            },
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn attachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                let filter_handle = local_filter_handle_from_strong(filter)?;
                let request = DvrFilterLinkRequest {
                    filter_id: filter_handle.object_id().0,
                    filter_generation: filter_handle.generation().0,
                };
                Ok(AidlMethodCall::DvrAttachFilter(request))
            },
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn detachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                let filter_handle = local_filter_handle_from_strong(filter)?;
                let request = DvrFilterLinkRequest {
                    filter_id: filter_handle.object_id().0,
                    filter_generation: filter_handle.generation().0,
                };
                Ok(AidlMethodCall::DvrDetachFilter(request))
            },
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn start(&self) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::DvrStart),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn stop(&self) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::DvrStop),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn flush(&self) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::DvrFlush),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(&self.runtime(), self.handle(), AidlMethodCall::DvrClose)
    }
    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || Ok(AidlMethodCall::DvrSetStatusCheckIntervalHint(milliseconds)),
            "DVR callback runtime is not connected in current tuner_hal2 scope",
        )
    }
}
