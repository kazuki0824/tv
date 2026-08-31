from pathlib import Path
import re


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    if s.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, got {s.count(old)}")
    p.write_text(s.replace(old, new, 1))

# #1: expose callback delivery failure as a typed accepted outcome instead of discarding Result.
replace_once(
    "tuner_hal2/service_runtime/src/frontend_ops.rs",
    """#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendOperationEventAcceptance {
    Accepted,
    DiscardedStale,
}
""",
    """#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendOperationEventAcceptance {
    Accepted,
    AcceptedCallbackFailure,
    DiscardedStale,
}
""",
)
old = """        match event {
            FrontendOperationEvent::Tune {
                notifier,
                notification,
            } => {
                let _ = notifier(frontend_id, operation_generation, notification);
            }
            FrontendOperationEvent::Scan {
                notifier,
                notification,
            } => {
                let _ = notifier(frontend_id, operation_generation, notification);
            }
        }
        Ok(FrontendOperationEventAcceptance::Accepted)
"""
new = """        let delivery = match event {
            FrontendOperationEvent::Tune {
                notifier,
                notification,
            } => notifier(frontend_id, operation_generation, notification),
            FrontendOperationEvent::Scan {
                notifier,
                notification,
            } => notifier(frontend_id, operation_generation, notification),
        };
        Ok(if delivery.is_ok() {
            FrontendOperationEventAcceptance::Accepted
        } else {
            // The AIDL notifier already commits the classified post-commit callback
            // failure through WorkerFailureClassifier -> PostCommitCallbackFailureTxn.
            // Preserve the committed tune/scan operation and expose that delivery
            // outcome explicitly instead of silently discarding it.
            FrontendOperationEventAcceptance::AcceptedCallbackFailure
        })
"""
replace_once("tuner_hal2/service_runtime/src/frontend_ops.rs", old, new)

# Make the internal delivery bridges consume every typed outcome explicitly. An
# internal acceptance error remains a worker error; callback delivery failure does not.
p = Path("tuner_hal2/service_runtime/src/frontend_worker_txn.rs")
s = p.read_text()
old = """fn deliver_committed_tune_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendTuneNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendTuneNotification,
) {
    let _ = FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Tune {
            notifier: Arc::clone(notifier),
            notification,
        },
    );
}

fn deliver_committed_scan_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendScanNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendScanNotification,
) {
    let _ = FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Scan {
            notifier: Arc::clone(notifier),
            notification,
        },
    );
}
"""
new = """fn deliver_committed_tune_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendTuneNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendTuneNotification,
) -> Result<(), HalError> {
    match FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Tune {
            notifier: Arc::clone(notifier),
            notification,
        },
    )? {
        crate::frontend_ops::FrontendOperationEventAcceptance::Accepted
        | crate::frontend_ops::FrontendOperationEventAcceptance::AcceptedCallbackFailure
        | crate::frontend_ops::FrontendOperationEventAcceptance::DiscardedStale => Ok(()),
    }
}

fn deliver_committed_scan_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendScanNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendScanNotification,
) -> Result<(), HalError> {
    match FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Scan {
            notifier: Arc::clone(notifier),
            notification,
        },
    )? {
        crate::frontend_ops::FrontendOperationEventAcceptance::Accepted
        | crate::frontend_ops::FrontendOperationEventAcceptance::AcceptedCallbackFailure
        | crate::frontend_ops::FrontendOperationEventAcceptance::DiscardedStale => Ok(()),
    }
}
"""
if s.count(old) != 1:
    raise SystemExit("frontend worker delivery helper anchor mismatch")
s = s.replace(old, new, 1)
# All production invocations are inside Result-returning orchestration. Consume the bridge result.
s = re.sub(r'(deliver_committed_tune_notification\(\n(?:[^;]|;(?!\n))*?\n\s*\))\s*;', r'\1?;', s)
s = re.sub(r'(deliver_committed_scan_notification\(\n(?:[^;]|;(?!\n))*?\n\s*\))\s*;', r'\1?;', s)
p.write_text(s)

# #3: make the canonical classifier the only public projection from generic worker terminal state.
p = Path("tuner_hal2/service_runtime/src/worker_failure_classifier.rs")
s = p.read_text()
s = s.replace(
    "use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};\n",
    "use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};\nuse crate::worker_runtime::WorkerTerminalResult;\n",
    1,
)
s = s.replace("pub(crate) enum WorkerFailureCategory", "pub enum WorkerFailureCategory", 1)
s = s.replace("pub(crate) struct WorkerFailureClassifier;", "pub struct WorkerFailureClassifier;", 1)
insert = r'''

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassifiedWorkerTerminalResult<T> {
    Normal(T),
    StopRequested,
    Failure {
        category: WorkerFailureCategory,
        error: HalError,
    },
}

impl<T> ClassifiedWorkerTerminalResult<T> {
    pub fn into_failure(self) -> Option<(WorkerFailureCategory, HalError)> {
        match self {
            Self::Failure { category, error } => Some((category, error)),
            Self::Normal(_) | Self::StopRequested => None,
        }
    }
}
'''
anchor = "pub struct WorkerFailureClassifier;"
s = s.replace(anchor, insert + "\n" + anchor, 1)
method = r'''
    pub(crate) fn classify_terminal<T>(
        result: WorkerTerminalResult<T>,
        panic_context: &'static str,
    ) -> ClassifiedWorkerTerminalResult<T> {
        match result {
            WorkerTerminalResult::Normal(value) => ClassifiedWorkerTerminalResult::Normal(value),
            WorkerTerminalResult::StopRequested => ClassifiedWorkerTerminalResult::StopRequested,
            WorkerTerminalResult::RuntimeFailure(error) => ClassifiedWorkerTerminalResult::Failure {
                category: Self::classify_unknown(&error),
                error,
            },
            WorkerTerminalResult::PanicOrJoinFailure => ClassifiedWorkerTerminalResult::Failure {
                category: Self::classify_join_failure(),
                error: HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    panic_context,
                ),
            },
        }
    }
'''
s = s.replace("impl WorkerFailureClassifier {\n", "impl WorkerFailureClassifier {\n" + method, 1)
p.write_text(s)

# Frontend termination no longer matches raw terminal variants itself.
p = Path("tuner_hal2/service_runtime/src/frontend_worker_termination_use_case.rs")
s = p.read_text()
s = s.replace("use crate::worker_runtime::WorkerTerminalResult;\n", "use crate::worker_failure_classifier::WorkerFailureClassifier;\n", 1)
old = """        let terminal_error = match event.into_terminal_result() {
            WorkerTerminalResult::Normal(()) | WorkerTerminalResult::StopRequested => None,
            WorkerTerminalResult::RuntimeFailure(error) => Some(error),
            WorkerTerminalResult::PanicOrJoinFailure => Some(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker panicked or could not be joined",
            )),
        };
"""
new = """        let terminal_error = WorkerFailureClassifier::classify_terminal(
            event.into_terminal_result(),
            "frontend worker panicked or could not be joined",
        )
        .into_failure()
        .map(|(_, error)| error);
"""
if s.count(old) != 1:
    raise SystemExit("frontend termination raw match anchor mismatch")
s = s.replace(old, new, 1)
# HalInternalKind is no longer used here.
s = s.replace("use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};", "use maleicacid_tuner_hal2_common::HalError;", 1)
p.write_text(s)

# Public WorkerRuntime users receive only classified terminal results.
p = Path("tuner_hal2/service_runtime/src/worker_runtime.rs")
s = p.read_text()
s = s.replace("pub enum WorkerTerminalResult<T>", "pub(crate) enum WorkerTerminalResult<T>", 1)
s = s.replace("    pub fn join(mut self) -> WorkerTerminalResult<T> {", "    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {", 1)
needle = "    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {"
pos = s.index(needle)
# add public classified join after the complete join method by locating next '\n    }\n' after its match body conservatively using known following test/module marker.
end_marker = "\n}\n\n#[cfg(test)]"
impl_end = s.index(end_marker, pos)
addition = r'''

    pub fn join_classified(self) -> crate::worker_failure_classifier::ClassifiedWorkerTerminalResult<T> {
        crate::worker_failure_classifier::WorkerFailureClassifier::classify_terminal(
            self.join(),
            "worker panicked or could not be joined",
        )
    }
'''
s = s[:impl_end] + addition + s[impl_end:]
p.write_text(s)

# Export classified public surface, not raw WorkerTerminalResult.
p = Path("tuner_hal2/service_runtime/src/lib.rs")
s = p.read_text()
s = s.replace(
    "pub use worker_runtime::{\n    WorkerHandle, WorkerRuntime, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,\n",
    "pub use worker_failure_classifier::{ClassifiedWorkerTerminalResult, WorkerFailureCategory};\npub use worker_runtime::{\n    WorkerHandle, WorkerRuntime, CLEANUP_RETRY_SCHEDULE_MS,\n",
    1,
)
p.write_text(s)

# DVR owner consumes the classified join result rather than reclassifying raw terminal variants.
p = Path("tuner_hal2/aidl_service/src/dvr_callback_delivery.rs")
s = p.read_text()
s = s.replace(
    "    DvrStatusPollSnapshot, WorkerRuntime, WorkerTerminalResult,\n",
    "    ClassifiedWorkerTerminalResult, DvrStatusPollSnapshot, WorkerRuntime,\n",
    1,
)
old = """    match notifier.worker.join() {
        WorkerTerminalResult::Normal(()) | WorkerTerminalResult::StopRequested => Ok(()),
        WorkerTerminalResult::RuntimeFailure(error) => Err(error),
        WorkerTerminalResult::PanicOrJoinFailure => Err(HalError::cleanup_failed(
            "DVR status notifier join",
            "DVR status notifier thread panicked",
        )),
    }
"""
new = """    match notifier.worker.join_classified() {
        ClassifiedWorkerTerminalResult::Normal(())
        | ClassifiedWorkerTerminalResult::StopRequested => Ok(()),
        ClassifiedWorkerTerminalResult::Failure { error, .. } => Err(error),
    }
"""
if s.count(old) != 1:
    raise SystemExit("DVR raw terminal match anchor mismatch")
s = s.replace(old, new, 1)
p.write_text(s)
