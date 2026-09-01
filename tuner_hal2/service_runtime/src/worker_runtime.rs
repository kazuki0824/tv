pub use maleicacid_tuner_hal2_control_core::{
    WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor,
    WorkerTerminalResult,
};

pub const CLEANUP_RETRY_SCHEDULE_MS: &[u64] = &[0, 10, 100, 1_000];
pub const CLEANUP_TERMINAL_DEADLINE_MS: u64 = 30_000;
pub const WORKER_IO_DEADLINE_MS: u64 = 2_000;
pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;

pub fn join_worker_classified<T>(
    worker: WorkerRuntime<T>,
) -> crate::worker_failure_classifier::ClassifiedWorkerTerminalResult<T> {
    crate::worker_failure_classifier::WorkerFailureClassifier::classify_terminal(
        worker.join(),
        "worker panicked or could not be joined",
    )
}
