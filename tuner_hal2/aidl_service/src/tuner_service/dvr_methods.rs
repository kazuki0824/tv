use super::support::local_filter_handle_from_strong;
use super::{
    build_dvr_configure_request, close_object_after_close_preflight, deliver_started_dvr_status,
    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, is_playback_dvr,
    start_dvr_status_notifier, status_from_hal_error, status_unknown_error,
    stop_dvr_status_notifier,
    tuner_queue_desc_from_snapshot, AidlMethodCall, AidlObjectGeneration, AidlObjectId,
    BinderResult, DvrAidlObject, DvrFilterLinkRequest, DvrSettings, IDvr, IFilter,
    ObjectQueryRequest, ObjectQueryResponse, Strong, TunerQueueDesc,
};
use crate::dvr_callback_delivery::{
    record_dvr_notifier_cleanup_outcome, record_dvr_post_commit_notification_outcome,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_service_runtime::DvrPostCommitNotificationPhase;

impl IDvr for DvrAidlObject {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        *queue = match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::DvrGetQueueDesc,
        )? {
            ObjectQueryResponse::QueueDescriptor(snapshot) => {
                tuner_queue_desc_from_snapshot(snapshot)
            }
            _ => {
                return Err(status_unknown_error(
                    "unexpected object query response for Dvr.getQueueDesc",
                ))
            }
        };
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
            |runtime, handle, dispatch_proof, request| {
                runtime.configure_dvr_runtime_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
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
            |runtime, handle, dispatch_proof, request| {
                runtime.attach_dvr_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    AidlObjectId(request.filter_id),
                    AidlObjectGeneration(request.filter_generation),
                    dispatch_proof,
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
            |runtime, handle, dispatch_proof, request| {
                runtime.detach_dvr_filter_for_object(
                    handle.object_id(),
                    handle.generation(),
                    AidlObjectId(request.filter_id),
                    AidlObjectGeneration(request.filter_generation),
                    dispatch_proof,
                )
            },
        )
    }
    fn start(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrStart,
            |runtime, handle, dispatch_proof| {
                runtime.start_dvr_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )?;
        record_dvr_post_commit_notification_outcome(
            &self.context(),
            self.handle(),
            DvrPostCommitNotificationPhase::InitialStatusDelivery,
            deliver_started_dvr_status(&self.context(), self.handle()),
        );
        let playback_kind = is_playback_dvr(&self.context(), self.handle());
        let worker_start_result = start_dvr_status_notifier(&self.context(), self.handle());
        let notifier_outcome = playback_kind
            .as_ref()
            .map(|_| ())
            .map_err(|error| error.clone())
            .and(worker_start_result.clone());
        record_dvr_post_commit_notification_outcome(
            &self.context(),
            self.handle(),
            DvrPostCommitNotificationPhase::StatusNotifierStart,
            notifier_outcome,
        );
        if playback_kind.is_err()
            || matches!(playback_kind, Ok(true)) && worker_start_result.is_err()
        {
            let shared_runtime = self.runtime();
            if let Ok(mut runtime) = shared_runtime.lock() {
                runtime.mark_service_critical();
            }
        }
        Ok(())
    }
    fn stop(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrStop,
            |runtime, handle, dispatch_proof| {
                runtime.stop_dvr_for_object(handle.object_id(), handle.generation(), dispatch_proof)
            },
        )?;
        record_dvr_notifier_cleanup_outcome(
            &self.context(),
            self.handle(),
            DvrPostCommitNotificationPhase::StatusNotifierStop,
            stop_dvr_status_notifier(&self.context(), self.handle()),
        );
        Ok(())
    }
    fn flush(&self) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrFlush,
            |runtime, handle, dispatch_proof| {
                runtime.flush_dvr_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(&self.context(), self.handle(), AidlMethodCall::DvrClose)
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
            |runtime, handle, dispatch_proof, interval_ms| {
                runtime.set_dvr_status_check_interval_for_object(
                    handle.object_id(),
                    handle.generation(),
                    interval_ms,
                    dispatch_proof,
                )
            },
        )
    }
}
