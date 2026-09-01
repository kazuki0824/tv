from pathlib import Path


def replace(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}")
    target.write_text(text.replace(old, new, 1))


replace(
    "tuner_hal2/demux/src/runtime/demux.rs",
    """use super::source_boundary::{
    apply_filter_source_boundary_change, connect_filter_source_boundary_change,
    SourceBoundaryReport,
};
const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
""",
    """use super::source_boundary::{
    apply_filter_source_boundary_change, connect_filter_source_boundary_change,
    SourceBoundaryReport,
};
mod filter_delay_delivery;
const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
""",
)

replace(
    "tuner_hal2/service_runtime/src/lib.rs",
    """pub use worker_runtime::{
    join_worker_classified, WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue,
    WorkerRuntimeSupervisor, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,
    CLEANUP_TERMINAL_DEADLINE_MS, WORKER_IO_DEADLINE_MS, WORKER_REAPER_DEADLINE_MS,
};
""",
    """pub use worker_runtime::{
    filter_delivery_wake_sequence, join_worker_classified, notify_filter_delivery_change,
    wait_filter_delivery_change, WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue,
    WorkerRuntimeSupervisor, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,
    CLEANUP_TERMINAL_DEADLINE_MS, WORKER_IO_DEADLINE_MS, WORKER_REAPER_DEADLINE_MS,
};
""",
)

replace(
    "tuner_hal2/aidl_service/src/filter_callback_delivery.rs",
    """use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FilterCallbackDeliveryDiagnosticPhase, FilterCallbackDeliveryDiagnosticRecord,
    FilterEventDelivery, FilterEventDeliverySnapshot, FilterEventDispatcher, TunerServiceRuntime,
};
""",
    """use maleicacid_tuner_hal2_service_runtime::{
    filter_delivery_wake_sequence, notify_filter_delivery_change, wait_filter_delivery_change,
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FilterCallbackDeliveryDiagnosticPhase, FilterCallbackDeliveryDiagnosticRecord,
    FilterEventDelivery, FilterEventDeliverySnapshot, FilterEventDispatcher, TunerServiceRuntime,
    WorkerRuntime,
};
""",
)

replace(
    "tuner_hal2/aidl_service/src/filter_callback_delivery.rs",
    """pub struct AidlFilterEventDispatcher {
    context: Weak<AidlServiceContext>,
}
""",
    """pub struct AidlFilterEventDispatcher {
    context: Weak<AidlServiceContext>,
    delay_worker: Option<WorkerRuntime<()>>,
}
""",
)

replace(
    "tuner_hal2/aidl_service/src/filter_callback_delivery.rs",
    """impl AidlFilterEventDispatcher {
    pub fn new(context: &SharedAidlServiceContext) -> Self {
        Self {
            context: Arc::downgrade(context),
        }
    }
}
""",
    """impl AidlFilterEventDispatcher {
    pub fn new(context: &SharedAidlServiceContext) -> Result<Self, HalError> {
        let worker_context = Arc::downgrade(context);
        let delay_worker = WorkerRuntime::spawn(
            \"maleicacid-filter-delay-delivery\".to_string(),
            0,
            1,
            move |stop| run_filter_delay_delivery(worker_context, stop),
            notify_filter_delivery_change,
        )
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!(\"filter delay delivery worker spawn failed: {error}\"),
            )
        })?;
        Ok(Self {
            context: Arc::downgrade(context),
            delay_worker: Some(delay_worker),
        })
    }

    fn without_worker(context: &SharedAidlServiceContext) -> Self {
        Self {
            context: Arc::downgrade(context),
            delay_worker: None,
        }
    }
}

impl Drop for AidlFilterEventDispatcher {
    fn drop(&mut self) {
        if let Some(worker) = self.delay_worker.as_ref() {
            worker.request_stop_and_wake();
            notify_filter_delivery_change();
        }
    }
}

pub(crate) fn dispatch_filter_event_snapshots(
    context: &SharedAidlServiceContext,
    snapshots: Vec<FilterEventDeliverySnapshot>,
) -> Result<(), HalError> {
    if snapshots.is_empty() {
        return Ok(());
    }
    let runtime = context.runtime();
    AidlFilterEventDispatcher::without_worker(context).dispatch(&runtime, snapshots)
}

fn run_filter_delay_delivery(
    weak_context: Weak<AidlServiceContext>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), HalError> {
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        let observed = filter_delivery_wake_sequence();
        let Some(context) = weak_context.upgrade() else {
            return Ok(());
        };
        let runtime = context.runtime();
        let (snapshots, deadline) = {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    \"service runtime lock poisoned while polling delayed filter events\",
                )
            })?;
            guard.poll_filter_delay_delivery()?
        };
        if !snapshots.is_empty() {
            let _recorded_failure = dispatch_filter_event_snapshots(&context, snapshots);
            continue;
        }
        drop(runtime);
        drop(context);
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        let _ = wait_filter_delivery_change(observed, deadline);
    }
}
""",
)

replace(
    "tuner_hal2/aidl_service/src/tuner_service.rs",
    """            runtime.install_filter_event_dispatcher(std::sync::Arc::new(
                AidlFilterEventDispatcher::new(&context),
            ))?;
""",
    """            runtime.install_filter_event_dispatcher(std::sync::Arc::new(
                AidlFilterEventDispatcher::new(&context)?,
            ))?;
""",
)

for method, call in [
    (
        "start",
        """execute_object_runtime_use_case(
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
        )""",
    ),
    (
        "stop",
        """execute_object_runtime_use_case(
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
        )""",
    ),
    (
        "flush",
        """execute_object_runtime_use_case(
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
        )""",
    ),
]:
    old = f"""    fn {method}(&self) -> BinderResult<()> {{
        {call}
    }}
"""
    new = f"""    fn {method}(&self) -> BinderResult<()> {{
        let result = {call};
        if result.is_ok() {{
            maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change();
        }}
        result
    }}
"""
    replace("tuner_hal2/aidl_service/src/filter_methods.rs", old, new)

replace(
    "tuner_hal2/aidl_service/src/filter_methods.rs",
    """    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.context(),
            self.handle(),
            AidlMethodCall::FilterClose,
        )
    }
""",
    """    fn close(&self) -> BinderResult<()> {
        let result = close_object_after_close_preflight(
            &self.context(),
            self.handle(),
            AidlMethodCall::FilterClose,
        );
        if result.is_ok() {
            maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change();
        }
        result
    }
""",
)

replace(
    "tuner_hal2/aidl_service/src/filter_methods.rs",
    """    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
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
""",
    """    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        let result = execute_object_runtime_use_case(
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
        );
        if result.is_ok() {
            maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change();
        }
        result
    }
""",
)

replace(
    "tuner_hal2/aidl_service/src/filter_methods.rs",
    """    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
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
""",
    """    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
        let result = execute_object_runtime_use_case_with_request_builder(
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
        );
        if result.is_ok() {
            maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change();
        }
        result
    }
""",
)

replace(
    "tuner_hal2/aidl_service/src/dvr_callback_delivery.rs",
    """use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};
""",
    """use crate::filter_callback_delivery::dispatch_filter_event_snapshots;
use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};
""",
)

replace(
    "tuner_hal2/aidl_service/src/dvr_callback_delivery.rs",
    """fn consume_playback_dvr_once(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            \"service runtime lock poisoned while consuming playback DVR data\",
        )
    })?;
    guard
        .consume_playback_dvr_for_object(handle.object_id(), handle.generation())
        .map(|_| ())
}
""",
    """fn consume_playback_dvr_once(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let events = {
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                \"service runtime lock poisoned while consuming playback DVR data\",
            )
        })?;
        let report =
            guard.consume_playback_dvr_for_object(handle.object_id(), handle.generation())?;
        guard.filter_event_delivery_snapshots_for_playback_report(&report)
    };
    maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change();
    let _recorded_failure = dispatch_filter_event_snapshots(context, events);
    Ok(())
}
""",
)

replace(
    "tuner_hal2/aidl_service/src/dvr_callback_delivery.rs",
    """        if initial_snapshot.is_playback {
            consume_playback_dvr_once(&runtime, handle)?;
        }
""",
    """        if initial_snapshot.is_playback {
            consume_playback_dvr_once(&context, handle)?;
        }
""",
)
