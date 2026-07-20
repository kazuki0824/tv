use super::support::local_filter_handle_from_strong;
use super::{
    build_dvr_configure_request, close_object_after_close_preflight, deliver_started_dvr_status,
    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, start_dvr_status_notifier,
    status_from_hal_error, status_unknown_error, stop_dvr_status_notifier,
    tuner_queue_desc_from_snapshot, AidlMethodCall, AidlObjectGeneration, AidlObjectId,
    BinderResult, DvrAidlObject, DvrFilterLinkRequest, DvrSettings, IDvr, IFilter,
    ObjectQueryRequest, ObjectQueryResponse, Strong, TunerQueueDesc,
};
use crate::dvr_callback_delivery::{
    record_dvr_notifier_cleanup_outcome, record_dvr_post_commit_notification_outcome,
};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidArgumentKind,
};
use crate::dvr_playback_worker::{
    playback_worker_cleanup_ownership_result, start_dvr_playback_worker,
    stop_dvr_playback_worker,
};
use maleicacid_tuner_hal2_service_runtime::{
    DvrPlaybackWorkerCleanupOperation, DvrPostCommitNotificationPhase, DvrStartTransition,
};

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
        let start_transition = execute_object_runtime_use_case(
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
        if let Err(worker_error) = start_dvr_playback_worker(&self.context(), self.handle(), start_transition) {
            if start_transition == DvrStartTransition::AlreadyStarted {
                return Err(status_from_hal_error(worker_error));
            }
            let runtime_handle = self.runtime();
            let (mut runtime, lock_error) = match runtime_handle.lock() {
                Ok(runtime) => (runtime, None),
                Err(poisoned) => (
                    poisoned.into_inner(),
                    Some(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while rolling back playback worker start failure",
                    )),
                ),
            };
            let rollback_result = runtime.rollback_started_dvr_after_playback_worker_failure(
                self.handle().object_id(),
                self.handle().generation(),
            );
            let mut error = worker_error;
            if let Some(lock_error) = lock_error {
                error = compose_primary_cleanup_failure(
                    "playback DVR worker start failed and runtime rollback lock was poisoned",
                    error,
                    lock_error,
                );
            }
            if let Err(rollback_error) = rollback_result {
                error = compose_primary_cleanup_failure(
                    "playback DVR worker start failed and runtime rollback failed",
                    error,
                    rollback_error,
                );
            }
            return Err(status_from_hal_error(error));
        }
        record_dvr_post_commit_notification_outcome(
            &self.context(),
            self.handle(),
            DvrPostCommitNotificationPhase::InitialStatusDelivery,
            deliver_started_dvr_status(&self.context(), self.handle()),
        );
        record_dvr_post_commit_notification_outcome(
            &self.context(),
            self.handle(),
            DvrPostCommitNotificationPhase::StatusNotifierStart,
            start_dvr_status_notifier(&self.context(), self.handle()),
        );
        Ok(())
    }
    fn stop(&self) -> BinderResult<()> {
        // Cancel and join the playback consumer before committing Stopped. This closes the race
        // where a live worker consumed FMQ data after the runtime stop commit.
        let playback_cleanup = stop_dvr_playback_worker(
            &self.context(),
            self.handle(),
            DvrPlaybackWorkerCleanupOperation::PublicStop,
        );
        if let Err(cleanup_error) = playback_worker_cleanup_ownership_result(&playback_cleanup) {
            record_dvr_post_commit_notification_outcome(
                &self.context(),
                self.handle(),
                DvrPostCommitNotificationPhase::PlaybackWorkerArtifactCleanup,
                Err(cleanup_error.clone()),
            );
            let runtime_handle = self.runtime();
            let (mut runtime, lock_error) = match runtime_handle.lock() {
                Ok(runtime) => (runtime, None),
                Err(poisoned) => (
                    poisoned.into_inner(),
                    Some(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while fail-closing playback worker cleanup failure",
                    )),
                ),
            };
            let failed_result = runtime.mark_dvr_failed_after_playback_worker_cleanup_failure(
                self.handle().object_id(),
                self.handle().generation(),
            );
            let mut error = cleanup_error;
            if let Some(lock_error) = lock_error {
                error = compose_primary_cleanup_failure(
                    "playback worker cleanup failed and runtime fail-close lock was poisoned",
                    error,
                    lock_error,
                );
            }
            if let Err(failed_error) = failed_result {
                error = compose_primary_cleanup_failure(
                    "playback worker cleanup failed and DVR failed-state commit failed",
                    error,
                    failed_error,
                );
            }
            return Err(status_from_hal_error(error));
        }
        let stop_result = execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::DvrStop,
            |runtime, handle, dispatch_proof| {
                runtime.stop_dvr_for_object(handle.object_id(), handle.generation(), dispatch_proof)
            },
        );
        if let Err(status) = stop_result {
            // Runtime stop did not commit. Restore the worker artifact for the still-started DVR.
            if let Err(restart_error) = start_dvr_playback_worker(
                &self.context(),
                self.handle(),
                DvrStartTransition::AlreadyStarted,
            ) {
                record_dvr_post_commit_notification_outcome(
                    &self.context(),
                    self.handle(),
                    DvrPostCommitNotificationPhase::PlaybackWorkerArtifactCleanup,
                    Err(restart_error),
                );
            }
            return Err(status);
        }
        if let Err(playback_error) = playback_cleanup.result() {
            record_dvr_post_commit_notification_outcome(
                &self.context(),
                self.handle(),
                DvrPostCommitNotificationPhase::PlaybackWorkerArtifactCleanup,
                Err(playback_error),
            );
        }
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
