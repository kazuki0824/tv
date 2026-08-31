from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    if s.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, got {s.count(old)}")
    p.write_text(s.replace(old, new, 1))

# Explicit fallible abort entries. Drop remains only a final fail-closed backstop.
replace_once(
    "tuner_hal2/demux/src/runtime/queue_runtime.rs",
    """    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        if self.reserved_bytes == 0 {
            return Err(protocol_error(
                "DVR queue transaction has an empty reservation",
            ));
        }
        let protocol_state = match self.direction {
            QueueTransactionDirection::Read | QueueTransactionDirection::Write => self.release()?,
        };
        if protocol_state == QueueEpochState::Closed {
            Err(protocol_error(
                "DVR queue transaction was closed before commit",
            ))
        } else {
            Ok(())
        }
    }
""",
    """    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        if self.reserved_bytes == 0 {
            return Err(protocol_error(
                "DVR queue transaction has an empty reservation",
            ));
        }
        let protocol_state = match self.direction {
            QueueTransactionDirection::Read | QueueTransactionDirection::Write => self.release()?,
        };
        if protocol_state == QueueEpochState::Closed {
            Err(protocol_error(
                "DVR queue transaction was closed before commit",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn abort(mut self) -> Result<(), QueueRuntimeError> {
        self.release().map(|_| ())
    }
""",
)
replace_once(
    "tuner_hal2/demux/src/runtime/queue_runtime.rs",
    """impl QueueEpochDrainTxn {
    fn rollback(&mut self) -> Result<(), QueueRuntimeError> {
""",
    """impl QueueEpochDrainTxn {
    fn rollback(&mut self) -> Result<(), QueueRuntimeError> {
""",
)
# Insert public typed abort after rollback impl body.
p = Path("tuner_hal2/demux/src/runtime/queue_runtime.rs")
s = p.read_text()
needle = """        self.protocol.drained.notify_all();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainCommitError {
"""
replacement = """        self.protocol.drained.notify_all();
        Ok(())
    }

    pub(crate) fn abort(mut self) -> Result<(), QueueRuntimeError> {
        self.rollback()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainCommitError {
"""
if s.count(needle) != 1:
    raise SystemExit("drain abort insertion anchor mismatch")
s = s.replace(needle, replacement, 1)
p.write_text(s)

# Make every expected pre-commit drain failure explicitly abort before returning.
p = Path("tuner_hal2/demux/src/runtime/queue_runtime.rs")
s = p.read_text()
old = """        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or(DvrQueueDrainCommitError::EpochCommit)?;
        if !Arc::ptr_eq(protocol, &drain.protocol) {
            return Err(DvrQueueDrainCommitError::EpochCommit);
        }
        let mut state = protocol
            .state
            .lock()
            .map_err(|_| DvrQueueDrainCommitError::EpochCommit)?;
        if state.state != QueueEpochState::Draining
            || state.epoch != drain.epoch
            || state.admitted_transaction_count != 0
        {
            return Err(DvrQueueDrainCommitError::EpochCommit);
        }

        // FmqQueue::clear() is failure-atomic: allocation happens before the
        // exact read, and a failed exact read leaves the read position intact.
        // Once it succeeds, only infallible in-memory epoch publication remains.
        let dropped_bytes = clear(self).map_err(|_| DvrQueueDrainCommitError::QueueClear)?;
"""
new = """        let Some(protocol) = self.dvr_epoch.as_ref() else {
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
"""
if s.count(old) != 1:
    raise SystemExit("drain commit preflight anchor mismatch")
s = s.replace(old, new, 1)
p.write_text(s)

# Write transactions explicitly abort when no payload commit is published.
p = Path("tuner_hal2/demux/src/runtime/demux.rs")
s = p.read_text()
old = """        if matches!(
            result.action,
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending
        ) {
            transaction
                .commit()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        }
        match result.action {
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending => Ok(result.bytes),
            FmqDeliveryAction::Overflow => Ok(0),
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
            }
        }
"""
new = """        match result.action {
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending => {
                transaction
                    .commit()
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
                Ok(result.bytes)
            }
            FmqDeliveryAction::Overflow => {
                transaction
                    .abort()
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
                Ok(0)
            }
            FmqDeliveryAction::RuntimeFailed(_) => {
                let abort_failed = transaction.abort().is_err();
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                let _ = abort_failed;
                Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
            }
        }
"""
if s.count(old) != 1:
    raise SystemExit("DVR write transaction anchor mismatch")
s = s.replace(old, new, 1)
# playback coordinate failure explicitly aborts the read admission.
old = """        let (queue_identity, queue_epoch) = match token.playback_coordinates() {
            Ok(coordinates) => coordinates,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
"""
new = """        let (queue_identity, queue_epoch) = match token.playback_coordinates() {
            Ok(coordinates) => coordinates,
            Err(_) => {
                let _ = token.abort();
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
"""
if s.count(old) != 1:
    raise SystemExit("playback coordinate anchor mismatch")
s = s.replace(old, new, 1)
# Add explicit owner abort entry for callers that fail after admission but before commit.
needle = """    pub fn commit_playback_queue_read(
        &mut self,
        mut txn: PlaybackQueueReadTxn,
    ) -> Result<TsInputOrigin, DemuxRuntimeError> {
"""
addition = """    pub fn abort_playback_queue_read(
        &mut self,
        mut txn: PlaybackQueueReadTxn,
    ) -> Result<(), DemuxRuntimeError> {
        let token = txn
            .token
            .take()
            .ok_or(DemuxRuntimeError::queue_runtime_failure(txn.dvr_id))?;
        token.abort().map_err(|_| {
            if let Some(dvr) = self.dvrs.get_mut(&txn.dvr_id) {
                dvr.mark_failed();
            }
            DemuxRuntimeError::queue_runtime_failure(txn.dvr_id)
        })
    }

"""
if s.count(needle) != 1:
    raise SystemExit("playback abort insertion anchor mismatch")
s = s.replace(needle, addition + needle, 1)
p.write_text(s)

# PlaybackConsumeTxn owns the read txn: explicitly abort on read failure/zero-read.
p = Path("tuner_hal2/service_runtime/src/playback_consume_txn.rs")
s = p.read_text()
old = """            let read = demux.read_playback_queue(
                &read_txn,
                &mut self.processing_buffer[..read_limit],
            )?;
            if read == 0 {
                return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id));
            }
            let origin = demux.commit_playback_queue_read(read_txn)?;
"""
new = """            let read = match demux.read_playback_queue(
                &read_txn,
                &mut self.processing_buffer[..read_limit],
            ) {
                Ok(read) => read,
                Err(primary) => {
                    let _ = demux.abort_playback_queue_read(read_txn);
                    return Err(primary);
                }
            };
            if read == 0 {
                demux.abort_playback_queue_read(read_txn)?;
                return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id));
            }
            let origin = demux.commit_playback_queue_read(read_txn)?;
"""
if s.count(old) != 1:
    raise SystemExit("playback consume abort anchor mismatch")
s = s.replace(old, new, 1)
p.write_text(s)
