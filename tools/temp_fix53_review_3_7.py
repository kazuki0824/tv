from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}")
    p.write_text(text.replace(old, new, 1))


# Preserve both the primary queue-boundary failure and a secondary explicit
# abort/rollback failure. Drop remains only the final fail-closed backstop.
replace_once(
    "tuner_hal2/demux/src/runtime/queue_runtime.rs",
    '''#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainCommitError {
    QueueClear,
    EpochCommit,
}
''',
    '''#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainCommitError {
    QueueClear,
    QueueClearRollbackFailed,
    EpochCommit,
    EpochCommitRollbackFailed,
}

impl DvrQueueDrainCommitError {
    fn after_abort(self, abort: Result<(), QueueRuntimeError>) -> Self {
        if abort.is_ok() {
            return self;
        }
        match self {
            Self::QueueClear | Self::QueueClearRollbackFailed => Self::QueueClearRollbackFailed,
            Self::EpochCommit | Self::EpochCommitRollbackFailed => Self::EpochCommitRollbackFailed,
        }
    }

    pub(crate) const fn rollback_failed(self) -> bool {
        matches!(
            self,
            Self::QueueClearRollbackFailed | Self::EpochCommitRollbackFailed
        )
    }
}
''',
)

old = '''        let Some(protocol) = self.dvr_epoch.as_ref() else {
            let _ = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit);
        };
        if !Arc::ptr_eq(protocol, &drain.protocol) {
            let _ = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit);
        }
        let mut state = match protocol.state.lock() {
            Ok(state) => state,
            Err(_) => {
                // The canonical lock itself is unavailable. Explicit abort is
                // attempted; Drop is only the final fail-closed backstop if that
                // abort cannot reacquire the poisoned state.
                let _ = drain.abort();
                return Err(DvrQueueDrainCommitError::EpochCommit);
            }
        };
        if state.state != QueueEpochState::Draining
            || state.epoch != drain.epoch
            || state.admitted_transaction_count != 0
        {
            drop(state);
            let _ = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit);
        }

        // FmqQueue::clear() is failure-atomic: allocation happens before the
        // exact read, and a failed exact read leaves the read position intact.
        // Once it succeeds, only infallible in-memory epoch publication remains.
        let dropped_bytes = match clear(self) {
            Ok(bytes) => bytes,
            Err(_) => {
                drop(state);
                let _ = drain.abort();
                return Err(DvrQueueDrainCommitError::QueueClear);
            }
        };
'''
new = '''        let Some(protocol) = self.dvr_epoch.as_ref() else {
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit.after_abort(abort));
        };
        if !Arc::ptr_eq(protocol, &drain.protocol) {
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit.after_abort(abort));
        }
        let mut state = match protocol.state.lock() {
            Ok(state) => state,
            Err(_) => {
                // The canonical lock itself is unavailable. Preserve an abort
                // failure as a typed secondary failure; Drop remains only the
                // final fail-closed backstop for an unconsumed authority.
                let abort = drain.abort();
                return Err(DvrQueueDrainCommitError::EpochCommit.after_abort(abort));
            }
        };
        if state.state != QueueEpochState::Draining
            || state.epoch != drain.epoch
            || state.admitted_transaction_count != 0
        {
            drop(state);
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::EpochCommit.after_abort(abort));
        }

        // FmqQueue::clear() is failure-atomic: allocation happens before the
        // exact read, and a failed exact read leaves the read position intact.
        // Once it succeeds, only infallible in-memory epoch publication remains.
        let dropped_bytes = match clear(self) {
            Ok(bytes) => bytes,
            Err(_) => {
                drop(state);
                let abort = drain.abort();
                return Err(DvrQueueDrainCommitError::QueueClear.after_abort(abort));
            }
        };
'''
replace_once("tuner_hal2/demux/src/runtime/queue_runtime.rs", old, new)

# Propagate the secondary rollback failure into the existing cleanup report as
# an explicit typed step while preserving the primary QueueClear/EpochCommit.
replace_once(
    "tuner_hal2/demux/src/runtime/demux.rs",
    '''pub enum DvrQueueCleanupStep {
    Prepare,
    QueueClear,
    QueueEpochCommit,
    RuntimeStateCommit,
''',
    '''pub enum DvrQueueCleanupStep {
    Prepare,
    QueueClear,
    QueueEpochCommit,
    QueueRollback,
    RuntimeStateCommit,
''',
)

replace_once(
    "tuner_hal2/demux/src/runtime/demux.rs",
    '''pub struct DvrQueueCleanupCommitError {
    failed_step: DvrQueueCleanupStep,
    error: DemuxRuntimeError,
}

impl DvrQueueCleanupCommitError {
    const fn new(failed_step: DvrQueueCleanupStep, error: DemuxRuntimeError) -> Self {
        Self { failed_step, error }
    }

    pub const fn failed_step(self) -> DvrQueueCleanupStep {
        self.failed_step
    }

    pub const fn error(self) -> DemuxRuntimeError {
        self.error
    }
}
''',
    '''pub struct DvrQueueCleanupCommitError {
    failed_step: DvrQueueCleanupStep,
    error: DemuxRuntimeError,
    rollback_failed: bool,
}

impl DvrQueueCleanupCommitError {
    const fn new(failed_step: DvrQueueCleanupStep, error: DemuxRuntimeError) -> Self {
        Self {
            failed_step,
            error,
            rollback_failed: false,
        }
    }

    const fn with_rollback_failure(
        failed_step: DvrQueueCleanupStep,
        error: DemuxRuntimeError,
    ) -> Self {
        Self {
            failed_step,
            error,
            rollback_failed: true,
        }
    }

    pub const fn failed_step(self) -> DvrQueueCleanupStep {
        self.failed_step
    }

    pub const fn error(self) -> DemuxRuntimeError {
        self.error
    }

    pub const fn rollback_failed(self) -> bool {
        self.rollback_failed
    }
}
''',
)

replace_once(
    "tuner_hal2/demux/src/runtime/demux.rs",
    '''        let queue_dropped_bytes = match queue_commit_result {
            Ok(dropped_bytes) => dropped_bytes,
            Err(DvrQueueDrainCommitError::QueueClear) => {
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueClear,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
            Err(DvrQueueDrainCommitError::EpochCommit) => {
                self.quarantine_dvr_runtime(dvr_id);
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
        };
''',
    '''        let queue_dropped_bytes = match queue_commit_result {
            Ok(dropped_bytes) => dropped_bytes,
            Err(DvrQueueDrainCommitError::QueueClear) => {
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueClear,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
            Err(DvrQueueDrainCommitError::QueueClearRollbackFailed) => {
                self.quarantine_dvr_runtime(dvr_id);
                return Err(DvrQueueCleanupCommitError::with_rollback_failure(
                    DvrQueueCleanupStep::QueueClear,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
            Err(DvrQueueDrainCommitError::EpochCommit) => {
                self.quarantine_dvr_runtime(dvr_id);
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
            Err(DvrQueueDrainCommitError::EpochCommitRollbackFailed) => {
                self.quarantine_dvr_runtime(dvr_id);
                return Err(DvrQueueCleanupCommitError::with_rollback_failure(
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
        };
''',
)

# Report the secondary failure without overwriting the primary failed step.
replace_once(
    "tuner_hal2/service_runtime/src/queue_cleanup_use_case.rs",
    '''fn record_dvr_queue_boundary_failure(
    report: &mut DvrQueueCleanupReport,
    error: DvrQueueCleanupCommitError,
) {
    match error.failed_step() {
''',
    '''fn record_dvr_queue_boundary_failure(
    report: &mut DvrQueueCleanupReport,
    error: DvrQueueCleanupCommitError,
) {
    match error.failed_step() {
''',
)
# Inject the secondary report after primary match but before skip of post-commit phases.
p = Path("tuner_hal2/service_runtime/src/queue_cleanup_use_case.rs")
text = p.read_text()
anchor = '''        failed_step => report.failed(failed_step, error.error().kind),
    }
    skip_dvr_queue_cleanup_steps(
'''
repl = '''        failed_step => report.failed(failed_step, error.error().kind),
    }
    if error.rollback_failed() {
        report.failed(
            DvrQueueCleanupStep::QueueRollback,
            DemuxRuntimeErrorKind::QueueRuntimeFailure,
        );
    } else {
        report.skipped(
            DvrQueueCleanupStep::QueueRollback,
            DvrQueueCleanupSkipReason::PrerequisiteFailed,
        );
    }
    skip_dvr_queue_cleanup_steps(
'''
if text.count(anchor) != 1:
    raise SystemExit("queue cleanup report anchor missing or ambiguous")
p.write_text(text.replace(anchor, repl, 1))

# Prepare failure never produced a drain authority, so QueueRollback is not runnable.
p = Path("tuner_hal2/service_runtime/src/queue_cleanup_use_case.rs")
text = p.read_text()
anchor = '''                    DvrQueueCleanupStep::QueueClear,
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DvrQueueCleanupStep::RuntimeStateCommit,
'''
repl = '''                    DvrQueueCleanupStep::QueueClear,
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DvrQueueCleanupStep::QueueRollback,
                    DvrQueueCleanupStep::RuntimeStateCommit,
'''
if text.count(anchor) != 1:
    raise SystemExit("prepare skip list anchor missing or ambiguous")
p.write_text(text.replace(anchor, repl, 1))
