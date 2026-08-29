use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use maleicacid_tuner_hal2_fmq::{FmqQueue, FmqQueueError};

use crate::packet_pipeline::PipelineGeneratedEvent;

#[derive(Debug)]
pub struct QueueDescriptorSnapshot {
    grantors: Vec<QueueGrantorDescriptorSnapshot>,
    fds: Vec<File>,
    ints: Vec<i32>,
    quantum: i32,
    flags: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueGrantorDescriptorSnapshot {
    fd_index: i32,
    offset: i32,
    extent: i64,
}

impl QueueDescriptorSnapshot {
    pub fn into_parts(
        self,
    ) -> (
        Vec<QueueGrantorDescriptorSnapshot>,
        Vec<File>,
        Vec<i32>,
        i32,
        i32,
    ) {
        (self.grantors, self.fds, self.ints, self.quantum, self.flags)
    }
}

impl QueueGrantorDescriptorSnapshot {
    pub const fn fd_index(self) -> i32 {
        self.fd_index
    }

    pub const fn offset(self) -> i32 {
        self.offset
    }

    pub const fn extent(self) -> i64 {
        self.extent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRuntimeErrorKind {
    InvalidCapacity,
    NativeCreateFailed,
    ExportTransient,
    DataPathFailure,
    StructuralDescriptor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueRuntimeError {
    pub kind: QueueRuntimeErrorKind,
    pub detail: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueueAvailabilitySnapshot {
    pub(crate) readable_bytes: usize,
    pub(crate) writable_bytes: usize,
}

impl QueueRuntimeError {
    pub(crate) const fn new(kind: QueueRuntimeErrorKind, detail: &'static str) -> Self {
        Self { kind, detail }
    }
}

#[derive(Clone)]
pub struct QueueRuntime {
    queue: Arc<FmqQueue>,
    playback_backing: Option<PlaybackQueueBacking>,
    capacity_bytes: usize,
    configure_event_flag: bool,
    wake_pending: Arc<AtomicBool>,
    dvr_epoch: Option<Arc<QueueEpochProtocol>>,
}

#[derive(Clone, Debug)]
struct PlaybackQueueBacking {
    queue_identity: u64,
}

static NEXT_PLAYBACK_QUEUE_IDENTITY: AtomicU64 = AtomicU64::new(1);

fn allocate_playback_queue_identity() -> Result<u64, QueueRuntimeError> {
    let mut current = NEXT_PLAYBACK_QUEUE_IDENTITY.load(Ordering::Acquire);
    loop {
        let next = current
            .checked_add(1)
            .ok_or_else(|| protocol_error("playback queue identity exhausted"))?;
        match NEXT_PLAYBACK_QUEUE_IDENTITY.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(current),
            Err(observed) => current = observed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueueEpochState {
    Open,
    Draining,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueTransactionDirection {
    Read,
    Write,
}

#[derive(Debug)]
struct QueueEpochProtocolState {
    state: QueueEpochState,
    epoch: u64,
    admitted_transaction_count: usize,
}

#[derive(Debug)]
/// Canonical DVR queue-epoch state owner. Tokens and drain transactions are
/// one-shot authorities issued by this owner and never carry an independent
/// epoch namespace.
pub(crate) struct QueueEpochProtocol {
    state: Mutex<QueueEpochProtocolState>,
    drained: Condvar,
    queue_identity: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct QueueEpochToken {
    protocol: Arc<QueueEpochProtocol>,
    queue_identity: Option<u64>,
    epoch: u64,
    direction: QueueTransactionDirection,
    reserved_bytes: usize,
    active: bool,
}

impl QueueEpochToken {
    fn release(&mut self) -> Result<QueueEpochState, QueueRuntimeError> {
        if !self.active {
            return Err(protocol_error(
                "DVR queue transaction was already consumed",
            ));
        }
        let mut state = self.protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while releasing a transaction")
        })?;
        if self.protocol.queue_identity != self.queue_identity
            || state.epoch != self.epoch
        {
            return Err(protocol_error(
                "DVR queue transaction identity or epoch changed before release",
            ));
        }
        let protocol_state = state.state;
        state.admitted_transaction_count = state
            .admitted_transaction_count
            .checked_sub(1)
            .ok_or_else(|| protocol_error("DVR queue transaction count underflow"))?;
        self.active = false;
        if state.admitted_transaction_count == 0 {
            self.protocol.drained.notify_all();
        }
        Ok(protocol_state)
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
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

    pub(crate) fn playback_coordinates(&self) -> Result<(u64, u64), QueueRuntimeError> {
        if !self.active || self.direction != QueueTransactionDirection::Read {
            return Err(protocol_error(
                "playback coordinates require an active DVR read transaction",
            ));
        }
        let queue_identity = self
            .queue_identity
            .ok_or_else(|| protocol_error("DVR queue is not a playback queue"))?;
        Ok((queue_identity, self.epoch))
    }
}

impl Drop for QueueEpochToken {
    fn drop(&mut self) {
        if self.active && self.release().is_err() {
            if let Ok(mut state) = self.protocol.state.lock() {
                state.state = QueueEpochState::Closed;
                self.protocol.drained.notify_all();
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct QueueEpochDrainTxn {
    protocol: Arc<QueueEpochProtocol>,
    epoch: u64,
    next_epoch: u64,
    active: bool,
}

impl QueueEpochDrainTxn {
    fn rollback(&mut self) -> Result<(), QueueRuntimeError> {
        if !self.active {
            return Ok(());
        }
        let mut state = self.protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while rolling back drain")
        })?;
        if state.state != QueueEpochState::Draining || state.epoch != self.epoch {
            return Err(protocol_error(
                "DVR queue drain state changed before rollback",
            ));
        }
        state.state = QueueEpochState::Open;
        self.active = false;
        self.protocol.drained.notify_all();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainCommitError {
    QueueClear,
    EpochCommit,
}

impl Drop for QueueEpochDrainTxn {
    fn drop(&mut self) {
        if self.active && self.rollback().is_err() {
            if let Ok(mut state) = self.protocol.state.lock() {
                state.state = QueueEpochState::Closed;
                self.protocol.drained.notify_all();
            }
        }
    }
}

fn protocol_error(detail: &'static str) -> QueueRuntimeError {
    QueueRuntimeError::new(QueueRuntimeErrorKind::StructuralDescriptor, detail)
}

pub(crate) struct QueueDescriptorExportHandle {
    queue: Arc<FmqQueue>,
}

impl fmt::Debug for QueueDescriptorExportHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueDescriptorExportHandle").finish()
    }
}

impl QueueDescriptorExportHandle {
    fn export_descriptor(&self) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
        export_queue_descriptor(&self.queue)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueDescriptorExportTarget {
    Filter { filter_id: i32 },
    Dvr { dvr_id: i32 },
}

#[derive(Debug)]
pub struct QueueDescriptorExportPlan {
    target: QueueDescriptorExportTarget,
    handle: QueueDescriptorExportHandle,
}

impl QueueDescriptorExportPlan {
    pub(crate) fn new(
        target: QueueDescriptorExportTarget,
        handle: QueueDescriptorExportHandle,
    ) -> Self {
        Self { target, handle }
    }

    pub const fn target(&self) -> QueueDescriptorExportTarget {
        self.target
    }

    pub fn export_descriptor(self) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
        self.handle.export_descriptor()
    }
}

impl fmt::Debug for QueueRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueRuntime")
            .field("capacity_bytes", &self.capacity_bytes)
            .field("configure_event_flag", &self.configure_event_flag)
            .finish()
    }
}

impl QueueRuntime {
    pub(crate) fn new_filter(
        buffer_size: i32,
        configure_event_flag: bool,
    ) -> Result<Self, QueueRuntimeError> {
        Self::new(buffer_size, configure_event_flag, false, false)
    }

    pub(crate) fn new_dvr(
        buffer_size: i32,
        configure_event_flag: bool,
        playback: bool,
    ) -> Result<Self, QueueRuntimeError> {
        Self::new(buffer_size, configure_event_flag, true, playback)
    }

    fn new(
        buffer_size: i32,
        configure_event_flag: bool,
        use_dvr_epoch_protocol: bool,
        playback: bool,
    ) -> Result<Self, QueueRuntimeError> {
        let capacity_bytes = usize::try_from(buffer_size).map_err(|_| {
            QueueRuntimeError::new(
                QueueRuntimeErrorKind::InvalidCapacity,
                "queue buffer size must be positive",
            )
        })?;
        if capacity_bytes == 0 {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::InvalidCapacity,
                "queue buffer size must be positive",
            ));
        }
        let queue = FmqQueue::create(capacity_bytes, configure_event_flag)
            .map_err(|err| map_create_error(err, "FMQ create failed"))?;
        let playback_backing = playback
            .then(allocate_playback_queue_identity)
            .transpose()?
            .map(|queue_identity| PlaybackQueueBacking { queue_identity });
        let queue_identity = playback_backing
            .as_ref()
            .map(|backing| backing.queue_identity);
        Ok(Self {
            queue: Arc::new(queue),
            playback_backing,
            capacity_bytes,
            configure_event_flag,
            wake_pending: Arc::new(AtomicBool::new(false)),
            dvr_epoch: use_dvr_epoch_protocol.then(|| {
                Arc::new(QueueEpochProtocol {
                    state: Mutex::new(QueueEpochProtocolState {
                        state: QueueEpochState::Open,
                        epoch: 0,
                        admitted_transaction_count: 0,
                    }),
                    drained: Condvar::new(),
                    queue_identity,
                })
            }),
        })
    }

    pub(crate) fn capacity_matches_buffer_size(&self, buffer_size: i32) -> bool {
        usize::try_from(buffer_size).ok() == Some(self.capacity_bytes)
    }

    pub(crate) const fn capacity_bytes(&self) -> usize {
        self.capacity_bytes
    }

    pub(crate) fn clear_contents(&self) -> Result<(), QueueRuntimeError> {
        let result = self
            .queue
            .clear()
            .map_err(|err| map_data_path_error(err, "FMQ clear failed"));
        if result.is_ok() {
            self.wake_pending.store(false, Ordering::Release);
        }
        result
    }

    pub(crate) fn commit_dvr_drain_with_queue_clear(
        &self,
        drain: QueueEpochDrainTxn,
    ) -> Result<usize, DvrQueueDrainCommitError> {
        self.commit_dvr_drain_with_queue_clear_operation(drain, |queue| {
            let dropped_bytes = queue.available_to_read()?;
            queue.clear_contents()?;
            Ok(dropped_bytes)
        })
    }

    fn commit_dvr_drain_with_queue_clear_operation<Clear>(
        &self,
        mut drain: QueueEpochDrainTxn,
        clear: Clear,
    ) -> Result<usize, DvrQueueDrainCommitError>
    where
        Clear: FnOnce(&Self) -> Result<usize, QueueRuntimeError>,
    {
        let protocol = self
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
        state.epoch = drain.next_epoch;
        state.state = QueueEpochState::Open;
        drain.active = false;
        self.wake_pending.store(false, Ordering::Release);
        protocol.drained.notify_all();
        Ok(dropped_bytes)
    }

    fn begin_dvr_transaction(
        &self,
        direction: QueueTransactionDirection,
        reserved_bytes: usize,
    ) -> Result<QueueEpochToken, QueueRuntimeError> {
        if reserved_bytes == 0 {
            return Err(protocol_error(
                "DVR queue transaction reservation must be positive",
            ));
        }
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVR queue epoch protocol is not installed"))?;
        let mut state = protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while admitting a transaction")
        })?;
        if state.state != QueueEpochState::Open {
            return Err(protocol_error("DVR queue epoch is draining or closed"));
        }
        state.admitted_transaction_count = state
            .admitted_transaction_count
            .checked_add(1)
            .ok_or_else(|| protocol_error("DVR queue transaction count overflow"))?;
        Ok(QueueEpochToken {
            protocol: Arc::clone(protocol),
            queue_identity: protocol.queue_identity,
            epoch: state.epoch,
            direction,
            reserved_bytes,
            active: true,
        })
    }

    pub(crate) fn begin_dvr_read(
        &self,
        reserved_bytes: usize,
    ) -> Result<QueueEpochToken, QueueRuntimeError> {
        self.begin_dvr_transaction(QueueTransactionDirection::Read, reserved_bytes)
    }

    pub(crate) fn begin_dvr_write(
        &self,
        reserved_bytes: usize,
    ) -> Result<QueueEpochToken, QueueRuntimeError> {
        self.begin_dvr_transaction(QueueTransactionDirection::Write, reserved_bytes)
    }

    pub(crate) fn begin_dvr_drain(&self) -> Result<QueueEpochDrainTxn, QueueRuntimeError> {
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVR queue epoch protocol is not installed"))?;
        let mut state = protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while beginning drain")
        })?;
        if state.state != QueueEpochState::Open {
            return Err(protocol_error("DVR queue epoch is not open"));
        }
        let next_epoch = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| protocol_error("DVR queue epoch exhausted"))?;
        state.state = QueueEpochState::Draining;
        while state.admitted_transaction_count != 0 {
            state = protocol.drained.wait(state).map_err(|_| {
                protocol_error("DVR queue epoch lock poisoned while waiting for drain")
            })?;
            if state.state != QueueEpochState::Draining {
                return Err(protocol_error(
                    "DVR queue epoch left draining state while waiting",
                ));
            }
        }
        Ok(QueueEpochDrainTxn {
            protocol: Arc::clone(protocol),
            epoch: state.epoch,
            next_epoch,
            active: true,
        })
    }

    pub(crate) fn playback_coordinates(&self) -> Result<(u64, u64), QueueRuntimeError> {
        let queue_identity = self
            .playback_backing
            .as_ref()
            .map(|backing| backing.queue_identity)
            .ok_or_else(|| protocol_error("DVR queue is not a playback queue"))?;
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVR queue epoch protocol is not installed"))?;
        let state = protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while reading playback coordinates")
        })?;
        Ok((queue_identity, state.epoch))
    }

    pub(crate) fn close_dvr_protocol(&self) -> Result<(), QueueRuntimeError> {
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVR queue epoch protocol is not installed"))?;
        let mut state = protocol.state.lock().map_err(|_| {
            protocol_error("DVR queue epoch lock poisoned while closing")
        })?;
        state.state = QueueEpochState::Closed;
        protocol.drained.notify_all();
        Ok(())
    }

    pub fn available_to_read(&self) -> Result<usize, QueueRuntimeError> {
        self.queue
            .available_to_read_result()
            .map_err(|err| map_data_path_error(err, "FMQ available_to_read failed"))
    }

    pub fn available_to_write(&self) -> Result<usize, QueueRuntimeError> {
        self.queue
            .available_to_write_result()
            .map_err(|err| map_data_path_error(err, "FMQ available_to_write failed"))
    }

    pub(crate) fn availability_snapshot(
        &self,
    ) -> Result<QueueAvailabilitySnapshot, QueueRuntimeError> {
        let readable_bytes = self
            .queue
            .current_fill()
            .map_err(|err| map_data_path_error(err, "FMQ fill snapshot failed"))?;
        let writable_bytes = self.capacity_bytes.checked_sub(readable_bytes).ok_or_else(|| {
            protocol_error("FMQ fill snapshot exceeds the configured queue capacity")
        })?;
        Ok(QueueAvailabilitySnapshot {
            readable_bytes,
            writable_bytes,
        })
    }

    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, QueueRuntimeError> {
        self.queue
            .read_into(data)
            .map_err(|err| map_data_path_error(err, "FMQ read failed"))
    }

    pub fn write_checked(&self, data: &[u8]) -> Result<usize, QueueRuntimeError> {
        self.queue
            .write_checked(data)
            .map_err(|err| map_data_path_error(err, "FMQ write failed"))
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), QueueRuntimeError> {
        let result = self
            .queue
            .wake(event_mask)
            .map_err(|err| map_data_path_error(err, "FMQ wake failed"));
        self.wake_pending.store(result.is_err(), Ordering::Release);
        result
    }

    pub fn retry_pending_wake(&self, event_mask: u32) -> Result<(), QueueRuntimeError> {
        if !self.wake_pending.load(Ordering::Acquire) {
            return Ok(());
        }
        self.wake(event_mask)
    }

    pub(crate) fn descriptor_export_handle(&self) -> QueueDescriptorExportHandle {
        QueueDescriptorExportHandle {
            queue: Arc::clone(&self.queue),
        }
    }
}

fn export_queue_descriptor(queue: &FmqQueue) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
    let grantor_count = queue
        .grantor_count_result()
        .map_err(|err| map_export_error(err, "FMQ grantor count export failed"))?;
    let mut grantors = Vec::with_capacity(grantor_count);
    for index in 0..grantor_count {
        let (fd_index, offset, extent) = queue
            .grantor_at_result(index)
            .map_err(|err| map_export_error(err, "FMQ grantor export failed"))?;
        grantors.push(QueueGrantorDescriptorSnapshot {
            fd_index,
            offset,
            extent,
        });
    }

    let fd_count = queue
        .fd_count_result()
        .map_err(|err| map_export_error(err, "FMQ fd count export failed"))?;
    let mut fds = Vec::with_capacity(fd_count);
    let mut fd_sizes = Vec::with_capacity(fd_count);
    for index in 0..fd_count {
        let fd = queue
            .dup_fd_at_result(index)
            .map_err(|err| map_export_error(err, "FMQ fd export failed"))?;
        let file = unsafe { File::from_raw_fd(fd) };
        let fd_size_u64 = file
            .metadata()
            .map_err(|_| {
                QueueRuntimeError::new(
                    QueueRuntimeErrorKind::StructuralDescriptor,
                    "FMQ descriptor fd metadata failed",
                )
            })?
            .len();
        let fd_size = i64::try_from(fd_size_u64).map_err(|_| {
            QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor fd size overflow",
            )
        })?;
        fd_sizes.push(fd_size);
        fds.push(file);
    }

    let int_count = queue
        .int_count_result()
        .map_err(|err| map_export_error(err, "FMQ int count export failed"))?;
    if int_count > 4 {
        return Err(QueueRuntimeError::new(
            QueueRuntimeErrorKind::StructuralDescriptor,
            "FMQ descriptor int count is invalid",
        ));
    }
    let mut ints = Vec::with_capacity(int_count);
    for index in 0..int_count {
        ints.push(
            queue
                .int_at_result(index)
                .map_err(|err| map_export_error(err, "FMQ int export failed"))?,
        );
    }

    validate_grantor_ranges_against_fd_sizes(&grantors, &fd_sizes)?;

    let quantum = queue
        .quantum_result()
        .map_err(|err| map_export_error(err, "FMQ quantum export failed"))?;
    if quantum <= 0 {
        return Err(QueueRuntimeError::new(
            QueueRuntimeErrorKind::StructuralDescriptor,
            "FMQ descriptor quantum is invalid",
        ));
    }
    let flags = queue
        .flags_result()
        .map_err(|err| map_export_error(err, "FMQ flags export failed"))?;

    Ok(QueueDescriptorSnapshot {
        grantors,
        fds,
        ints,
        quantum,
        flags,
    })
}

fn validate_grantor_ranges_against_fd_sizes(
    grantors: &[QueueGrantorDescriptorSnapshot],
    fd_sizes: &[i64],
) -> Result<(), QueueRuntimeError> {
    for grantor in grantors {
        if grantor.fd_index < 0 || grantor.fd_index as usize >= fd_sizes.len() {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor grantor fd index is out of range",
            ));
        }
        if grantor.offset < 0 || grantor.extent <= 0 {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor grantor range is invalid",
            ));
        }
        let Some(end) = i64::from(grantor.offset).checked_add(grantor.extent) else {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor grantor range overflowed",
            ));
        };
        let fd_size = fd_sizes[grantor.fd_index as usize];
        if fd_size > 0 && end > fd_size {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor grantor range exceeds fd size",
            ));
        }
    }
    Ok(())
}

fn map_create_error(err: FmqQueueError, detail: &'static str) -> QueueRuntimeError {
    let kind = match err {
        FmqQueueError::NativeCreateFailed => QueueRuntimeErrorKind::NativeCreateFailed,
        _ => QueueRuntimeErrorKind::DataPathFailure,
    };
    QueueRuntimeError::new(kind, detail)
}

fn map_export_error(err: FmqQueueError, detail: &'static str) -> QueueRuntimeError {
    let kind = match err {
        FmqQueueError::DescriptorFdDupFailed
        | FmqQueueError::DescriptorGrantorUnavailable
        | FmqQueueError::DescriptorIntUnavailable => QueueRuntimeErrorKind::ExportTransient,
        _ => QueueRuntimeErrorKind::StructuralDescriptor,
    };
    QueueRuntimeError::new(kind, detail)
}

fn map_data_path_error(err: FmqQueueError, detail: &'static str) -> QueueRuntimeError {
    let kind = match err {
        FmqQueueError::NativeReadZero
        | FmqQueueError::NativeClearBufferAllocationFailed
        | FmqQueueError::NativeClearReadFailed
        | FmqQueueError::NativeWriteFailed
        | FmqQueueError::NativeWriteInvalidArgument
        | FmqQueueError::NativeWakeFailed => QueueRuntimeErrorKind::DataPathFailure,
        _ => QueueRuntimeErrorKind::StructuralDescriptor,
    };
    QueueRuntimeError::new(kind, detail)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GateState {
    Open,
    Draining,
    Closed,
}

#[derive(Debug)]
struct GateData {
    state: GateState,
    filter_delivery_generation: u64,
    parser_state_generation: u64,
    admitted_producer_count: usize,
    pending_events: VecDeque<PipelineGeneratedEvent>,
    pending_event_capacity: usize,
    record_output_byte_offset: u64,
}

#[derive(Debug)]
struct GateInner {
    data: Mutex<GateData>,
    drained: Condvar,
}

#[derive(Clone, Debug)]
pub(crate) struct FilterProducerDrainGate {
    inner: Arc<GateInner>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilterDrainBoundary {
    Flush,
    Reconfigure,
}

#[derive(Debug)]
pub(crate) struct FilterProducerPermit {
    inner: Arc<GateInner>,
    delivery_generation: u64,
    active: bool,
}

#[derive(Debug)]
pub(crate) struct FilterDrainTxn {
    inner: Arc<GateInner>,
    boundary: FilterDrainBoundary,
    delivery_generation: u64,
    parser_generation: u64,
    next_delivery_generation: u64,
    next_parser_generation: u64,
    active: bool,
}

fn gate_error(detail: &'static str) -> QueueRuntimeError {
    QueueRuntimeError::new(QueueRuntimeErrorKind::StructuralDescriptor, detail)
}

impl FilterProducerDrainGate {
    pub(crate) fn new(pending_event_capacity: usize) -> Result<Self, QueueRuntimeError> {
        if pending_event_capacity == 0 {
            return Err(gate_error(
                "filter producer gate pending event capacity must be positive",
            ));
        }
        let mut pending_events = VecDeque::new();
        pending_events
            .try_reserve_exact(pending_event_capacity)
            .map_err(|_| gate_error("filter producer gate pending event reservation failed"))?;
        Ok(Self {
            inner: Arc::new(GateInner {
                data: Mutex::new(GateData {
                    state: GateState::Open,
                    filter_delivery_generation: 0,
                    parser_state_generation: 0,
                    admitted_producer_count: 0,
                    pending_events,
                    pending_event_capacity,
                    record_output_byte_offset: 0,
                }),
                drained: Condvar::new(),
            }),
        })
    }

    pub(crate) fn begin_producer(&self) -> Result<FilterProducerPermit, QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while admitting"))?;
        if data.state != GateState::Open {
            return Err(gate_error("filter producer gate is draining or closed"));
        }
        data.admitted_producer_count = data
            .admitted_producer_count
            .checked_add(1)
            .ok_or_else(|| gate_error("filter producer permit count overflow"))?;
        Ok(FilterProducerPermit {
            inner: Arc::clone(&self.inner),
            delivery_generation: data.filter_delivery_generation,
            active: true,
        })
    }

    pub(crate) fn begin_drain(
        &self,
        boundary: FilterDrainBoundary,
    ) -> Result<FilterDrainTxn, QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while draining"))?;
        if data.state != GateState::Open {
            return Err(gate_error("filter producer gate is not open"));
        }
        let next_parser_generation = data
            .parser_state_generation
            .checked_add(1)
            .ok_or_else(|| gate_error("filter parser generation exhausted"))?;
        let next_delivery_generation = match boundary {
            FilterDrainBoundary::Flush => data.filter_delivery_generation,
            FilterDrainBoundary::Reconfigure => data
                .filter_delivery_generation
                .checked_add(1)
                .ok_or_else(|| gate_error("filter delivery generation exhausted"))?,
        };
        data.state = GateState::Draining;
        while data.admitted_producer_count != 0 {
            data = self
                .inner
                .drained
                .wait(data)
                .map_err(|_| gate_error("filter producer gate lock poisoned while waiting"))?;
            if data.state != GateState::Draining {
                return Err(gate_error(
                    "filter producer gate left draining state while waiting",
                ));
            }
        }
        Ok(FilterDrainTxn {
            inner: Arc::clone(&self.inner),
            boundary,
            delivery_generation: data.filter_delivery_generation,
            parser_generation: data.parser_state_generation,
            next_delivery_generation,
            next_parser_generation,
            active: true,
        })
    }

    pub(crate) fn take_pending_events(
        &self,
    ) -> Result<Vec<PipelineGeneratedEvent>, QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while taking events"))?;
        match data.state {
            GateState::Open => Ok(data.pending_events.drain(..).collect()),
            GateState::Draining => Ok(Vec::new()),
            GateState::Closed => Err(gate_error(
                "filter producer gate is closed while taking events",
            )),
        }
    }

    pub(crate) fn close(&self) -> Result<(), QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while closing"))?;
        data.state = GateState::Closed;
        data.pending_events.clear();
        self.inner.drained.notify_all();
        Ok(())
    }
}

impl FilterProducerPermit {
    pub(crate) fn record_output_byte_offset(&self) -> Result<u64, QueueRuntimeError> {
        if !self.active {
            return Err(gate_error("filter producer permit was already consumed"));
        }
        let data = self.inner.data.lock().map_err(|_| {
            gate_error("filter producer gate lock poisoned while reading record offset")
        })?;
        if data.state == GateState::Closed
            || data.filter_delivery_generation != self.delivery_generation
        {
            return Err(gate_error("filter producer permit is stale"));
        }
        Ok(data.record_output_byte_offset)
    }

    pub(crate) fn commit_record_output(
        mut self,
        committed_bytes: usize,
        event: Option<PipelineGeneratedEvent>,
    ) -> Result<(), QueueRuntimeError> {
        if !self.active || committed_bytes == 0 {
            return Err(gate_error("record output commit is invalid"));
        }
        let committed_bytes = u64::try_from(committed_bytes)
            .map_err(|_| gate_error("record output byte count is out of range"))?;
        let mut data = self.inner.data.lock().map_err(|_| {
            gate_error("filter producer gate lock poisoned while committing record output")
        })?;
        if data.filter_delivery_generation != self.delivery_generation {
            return Err(gate_error("filter producer permit generation changed"));
        }
        let next_offset = data
            .record_output_byte_offset
            .checked_add(committed_bytes)
            .ok_or_else(|| gate_error("record output byte offset exhausted"))?;
        let event_queue_full = event.is_some()
            && data.pending_events.len() >= data.pending_event_capacity;
        data.record_output_byte_offset = next_offset;
        if !event_queue_full {
            if let Some(event) = event {
                data.pending_events.push_back(event);
            }
        }
        let gate_state = data.state;
        data.admitted_producer_count = data
            .admitted_producer_count
            .checked_sub(1)
            .ok_or_else(|| gate_error("filter producer permit count underflow"))?;
        self.active = false;
        if data.admitted_producer_count == 0 {
            self.inner.drained.notify_all();
        }
        if event_queue_full {
            Err(gate_error("filter producer pending event queue is full"))
        } else if gate_state == GateState::Closed {
            Err(gate_error("filter producer gate closed before record output commit"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn enqueue_event(
        &mut self,
        event: PipelineGeneratedEvent,
    ) -> Result<(), QueueRuntimeError> {
        if !self.active {
            return Err(gate_error("filter producer permit was already consumed"));
        }
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while queueing event"))?;
        if data.state == GateState::Closed
            || data.filter_delivery_generation != self.delivery_generation
        {
            return Err(gate_error("filter producer permit is stale"));
        }
        if data.pending_events.len() >= data.pending_event_capacity {
            return Err(gate_error("filter producer pending event queue is full"));
        }
        data.pending_events.push_back(event);
        Ok(())
    }

    fn release(&mut self) -> Result<GateState, QueueRuntimeError> {
        if !self.active {
            return Err(gate_error("filter producer permit was already consumed"));
        }
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while releasing"))?;
        if data.filter_delivery_generation != self.delivery_generation {
            return Err(gate_error("filter producer permit generation changed"));
        }
        let gate_state = data.state;
        data.admitted_producer_count = data
            .admitted_producer_count
            .checked_sub(1)
            .ok_or_else(|| gate_error("filter producer permit count underflow"))?;
        self.active = false;
        if data.admitted_producer_count == 0 {
            self.inner.drained.notify_all();
        }
        Ok(gate_state)
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        if self.release()? == GateState::Closed {
            Err(gate_error("filter producer gate closed before commit"))
        } else {
            Ok(())
        }
    }
}

impl Drop for FilterProducerPermit {
    fn drop(&mut self) {
        if self.active && self.release().is_err() {
            if let Ok(mut data) = self.inner.data.lock() {
                data.state = GateState::Closed;
                data.pending_events.clear();
                self.inner.drained.notify_all();
            }
        }
    }
}

impl FilterDrainTxn {
    pub(crate) fn take_pending_events(
        &mut self,
    ) -> Result<Vec<PipelineGeneratedEvent>, QueueRuntimeError> {
        if !self.active {
            return Err(gate_error("filter producer drain was already consumed"));
        }
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while draining events"))?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error(
                "filter producer drain state changed before draining events",
            ));
        }
        Ok(data.pending_events.drain(..).collect())
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while committing"))?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error("filter producer drain state changed before commit"));
        }
        match self.boundary {
            FilterDrainBoundary::Flush | FilterDrainBoundary::Reconfigure => {
                data.pending_events.clear();
            }
        }
        data.filter_delivery_generation = self.next_delivery_generation;
        data.parser_state_generation = self.next_parser_generation;
        data.state = GateState::Open;
        self.active = false;
        self.inner.drained.notify_all();
        Ok(())
    }

    pub(crate) fn commit_and_take_pending_events(
        mut self,
    ) -> Result<Vec<PipelineGeneratedEvent>, QueueRuntimeError> {
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while committing"))?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error("filter producer drain state changed before commit"));
        }
        let pending_events = data.pending_events.drain(..).collect();
        data.filter_delivery_generation = self.next_delivery_generation;
        data.parser_state_generation = self.next_parser_generation;
        data.state = GateState::Open;
        self.active = false;
        self.inner.drained.notify_all();
        Ok(pending_events)
    }

    fn rollback(&mut self) -> Result<(), QueueRuntimeError> {
        if !self.active {
            return Ok(());
        }
        let mut data = self
            .inner
            .data
            .lock()
            .map_err(|_| gate_error("filter producer gate lock poisoned while rolling back"))?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
        {
            return Err(gate_error("filter producer drain state changed before rollback"));
        }
        data.state = GateState::Open;
        self.active = false;
        self.inner.drained.notify_all();
        Ok(())
    }
}

impl Drop for FilterDrainTxn {
    fn drop(&mut self) {
        if self.active && self.rollback().is_err() {
            if let Ok(mut data) = self.inner.data.lock() {
                data.state = GateState::Closed;
                data.pending_events.clear();
                self.inner.drained.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod dvr_queue_cleanup_tests {
    use super::*;

    #[test]
    fn failed_queue_clear_preserves_content_epoch_and_open_state() {
        let queue = QueueRuntime::new_dvr(64, false, true).expect("DVR queue must open");
        let payload = [0x11, 0x22, 0x33, 0x44];
        assert_eq!(queue.write_checked(&payload), Ok(payload.len()));
        let coordinates_before = queue
            .playback_coordinates()
            .expect("playback coordinates must be available");
        let drain = queue
            .begin_dvr_drain()
            .expect("queue drain must begin without admitted transactions");

        let error = queue
            .commit_dvr_drain_with_queue_clear_operation(drain, |_| {
                Err(QueueRuntimeError::new(
                    QueueRuntimeErrorKind::DataPathFailure,
                    "injected FMQ clear failure",
                ))
            })
            .expect_err("injected clear failure must fail the cleanup boundary");

        assert_eq!(error, DvrQueueDrainCommitError::QueueClear);
        assert_eq!(queue.available_to_read(), Ok(payload.len()));
        assert_eq!(queue.playback_coordinates(), Ok(coordinates_before));
        let transaction = queue
            .begin_dvr_read(1)
            .expect("failed precommit must reopen the old epoch");
        transaction
            .commit()
            .expect("old-epoch transaction must remain usable");
    }

    #[test]
    fn rejected_epoch_preflight_does_not_clear_queue_content() {
        let target = QueueRuntime::new_dvr(64, false, true).expect("target DVR queue must open");
        let other = QueueRuntime::new_dvr(64, false, true).expect("other DVR queue must open");
        let payload = [0x21, 0x32, 0x43];
        assert_eq!(target.write_checked(&payload), Ok(payload.len()));
        let other_drain = other
            .begin_dvr_drain()
            .expect("other queue drain must begin without admitted transactions");

        let error = target
            .commit_dvr_drain_with_queue_clear(other_drain)
            .expect_err("a drain from another queue must fail preflight");

        assert_eq!(error, DvrQueueDrainCommitError::EpochCommit);
        assert_eq!(target.available_to_read(), Ok(payload.len()));
        let transaction = other
            .begin_dvr_read(1)
            .expect("rejected drain must reopen its original queue");
        transaction
            .commit()
            .expect("the original queue epoch must remain usable");
    }

    #[test]
    fn successful_queue_clear_publishes_the_next_epoch_after_content_is_gone() {
        let queue = QueueRuntime::new_dvr(64, false, true).expect("DVR queue must open");
        let payload = [0x55, 0x66, 0x77];
        assert_eq!(queue.write_checked(&payload), Ok(payload.len()));
        let (_, epoch_before) = queue
            .playback_coordinates()
            .expect("playback coordinates must be available");
        let drain = queue
            .begin_dvr_drain()
            .expect("queue drain must begin without admitted transactions");

        let dropped_bytes = queue
            .commit_dvr_drain_with_queue_clear(drain)
            .expect("queue clear and epoch commit must succeed together");

        assert_eq!(dropped_bytes, payload.len());
        assert_eq!(queue.available_to_read(), Ok(0));
        let (_, epoch_after) = queue
            .playback_coordinates()
            .expect("playback coordinates must remain available");
        assert_eq!(epoch_after, epoch_before + 1);
    }
}
