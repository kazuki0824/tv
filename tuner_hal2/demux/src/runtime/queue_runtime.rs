use maleicacid_tuner_hal2_common::{LockPoisonDiagnostic, PoisonTrackedMutex, RuntimeLockKind};
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
pub enum QueueRuntimeLockKind {
    FilterProducerDrainGateData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRuntimeErrorKind {
    InvalidCapacity,
    NativeCreateFailed,
    ExportTransient,
    DataPathFailure,
    StructuralDescriptor,
    EpochLockPoisoned(LockPoisonDiagnostic),
    GateCleanupFailed {
        producer_release: bool,
        drain_rollback: bool,
    },
    GateLockPoisoned {
        lock: QueueRuntimeLockKind,
        poison_count: u64,
        counter_saturated: bool,
        producer_release: bool,
        drain_rollback: bool,
    },
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
            .ok_or_else(|| protocol_error("再生キュー識別子を発行できません"))?;
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
/// DVR queue-epoch stateを所有する正規owner。
/// tokenとdrain transactionはこのownerが発行する一回限り権限であり、独立epoch namespaceを持たない。
pub(crate) struct QueueEpochProtocol {
    state: PoisonTrackedMutex<QueueEpochProtocolState>,
    drained: Condvar,
    queue_identity: Option<u64>,
}

impl QueueEpochProtocol {
    fn fail_close_unconsumed_authority(&self) {
        match self.state.lock() {
            Ok(mut state) => state.state = QueueEpochState::Closed,
            Err(_) => {
                // poisonを解除せず、以後の全受付で既存の型付きlock失敗を返す。
                eprintln!(
                    "DVRキューエポックの未消費権限を破棄しました: queue={:?}, ロック汚染",
                    self.queue_identity
                );
            }
        }
        self.drained.notify_all();
    }
}

#[derive(Debug)]
#[must_use = "この準備済み一回限り権限は型付き完了入口で消費する必要があります"]
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
                "DVRキューのトランザクションは既に消費されています",
            ));
        }
        let mut state = self.protocol.state.lock().map_err(epoch_poison)?;
        if self.protocol.queue_identity != self.queue_identity || state.epoch != self.epoch {
            return Err(protocol_error(
                "解放前にDVRキューの識別子またはエポックが変化しました",
            ));
        }
        let protocol_state = state.state;
        state.admitted_transaction_count = state
            .admitted_transaction_count
            .checked_sub(1)
            .ok_or_else(|| protocol_error("DVRキューのトランザクション数が下限を下回りました"))?;
        self.active = false;
        if state.admitted_transaction_count == 0 {
            self.protocol.drained.notify_all();
        }
        Ok(protocol_state)
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        if self.reserved_bytes == 0 {
            return Err(protocol_error("DVRキューの予約サイズが0です"));
        }
        let protocol_state = match self.direction {
            QueueTransactionDirection::Read | QueueTransactionDirection::Write => self.release()?,
        };
        if protocol_state == QueueEpochState::Closed {
            Err(protocol_error("確定前にDVRキューが閉じられました"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn abort(mut self) -> Result<(), QueueRuntimeError> {
        self.release().map(|_| ())
    }

    pub(crate) fn playback_coordinates(&self) -> Result<(u64, u64), QueueRuntimeError> {
        if !self.active || self.direction != QueueTransactionDirection::Read {
            return Err(protocol_error(
                "再生位置の取得には有効なDVR読み取りトランザクションが必要です",
            ));
        }
        let queue_identity = self
            .queue_identity
            .ok_or_else(|| protocol_error("DVRキューは再生用ではありません"))?;
        Ok((queue_identity, self.epoch))
    }
}

impl Drop for QueueEpochToken {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.protocol.fail_close_unconsumed_authority();
        }
    }
}

#[derive(Debug)]
#[must_use = "この準備済み一回限り権限は型付き完了入口で消費する必要があります"]
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
        let mut state = self.protocol.state.lock().map_err(epoch_poison)?;
        if state.state != QueueEpochState::Draining || state.epoch != self.epoch {
            return Err(protocol_error(
                "巻き戻し前にDVRキュー排出状態が変化しました",
            ));
        }
        state.state = QueueEpochState::Open;
        self.active = false;
        self.protocol.drained.notify_all();
        Ok(())
    }

    pub(crate) fn abort(mut self) -> Result<(), QueueRuntimeError> {
        self.rollback()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DvrQueueDrainStep {
    QueueClear,
    EpochCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DvrQueueDrainCommitError {
    pub step: DvrQueueDrainStep,
    pub primary: QueueRuntimeError,
    pub rollback: Option<QueueRuntimeError>,
}

impl DvrQueueDrainCommitError {
    fn after_abort(
        step: DvrQueueDrainStep,
        primary: QueueRuntimeError,
        abort: Result<(), QueueRuntimeError>,
    ) -> Self {
        Self {
            step,
            primary,
            rollback: abort.err(),
        }
    }
}

impl Drop for QueueEpochDrainTxn {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.protocol.fail_close_unconsumed_authority();
        }
    }
}

fn epoch_poison(poison: LockPoisonDiagnostic) -> QueueRuntimeError {
    QueueRuntimeError::new(
        QueueRuntimeErrorKind::EpochLockPoisoned(poison),
        "DVRキューエポックのロックが汚染されています",
    )
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
                "キューバッファーサイズは正数である必要があります",
            )
        })?;
        if capacity_bytes == 0 {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::InvalidCapacity,
                "キューバッファーサイズは正数である必要があります",
            ));
        }
        let queue = FmqQueue::create(capacity_bytes, configure_event_flag)
            .map_err(|err| map_create_error(err, "FMQの作成に失敗しました"))?;
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
                    state: PoisonTrackedMutex::new(
                        QueueEpochProtocolState {
                            state: QueueEpochState::Open,
                            epoch: 0,
                            admitted_transaction_count: 0,
                        },
                        RuntimeLockKind::DvrQueueEpoch { queue_identity },
                    ),
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
            .map_err(|err| map_data_path_error(err, "FMQの消去に失敗しました"));
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
        let Some(protocol) = self.dvr_epoch.as_ref() else {
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::after_abort(
                DvrQueueDrainStep::EpochCommit,
                protocol_error("DVRキューエポックの確定が拒否されました"),
                abort,
            ));
        };
        if !Arc::ptr_eq(protocol, &drain.protocol) {
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::after_abort(
                DvrQueueDrainStep::EpochCommit,
                protocol_error("DVRキューエポックの確定が拒否されました"),
                abort,
            ));
        }
        let mut state = match protocol.state.lock() {
            Ok(state) => state,
            Err(poison) => {
                // 汚染と取消し失敗を両方保持し、未消費権限の破棄時も受付を再開しない。
                let abort = drain.abort();
                return Err(DvrQueueDrainCommitError::after_abort(
                    DvrQueueDrainStep::EpochCommit,
                    epoch_poison(poison),
                    abort,
                ));
            }
        };
        if state.state != QueueEpochState::Draining
            || state.epoch != drain.epoch
            || state.admitted_transaction_count != 0
        {
            drop(state);
            let abort = drain.abort();
            return Err(DvrQueueDrainCommitError::after_abort(
                DvrQueueDrainStep::EpochCommit,
                protocol_error("DVRキューエポックの確定が拒否されました"),
                abort,
            ));
        }

        // FmqQueue::clear()はfailure-atomicで、allocationをexact readより先に行い、
        // exact read失敗時はread positionを維持する。
        // 成功後に残るのは失敗しないmemory内epoch publicationだけである。
        let dropped_bytes = match clear(self) {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(state);
                let abort = drain.abort();
                return Err(DvrQueueDrainCommitError::after_abort(
                    DvrQueueDrainStep::QueueClear,
                    error,
                    abort,
                ));
            }
        };
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
                "DVRキューの予約サイズは正数である必要があります",
            ));
        }
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVRキューのエポック制御が設定されていません"))?;
        let mut state = protocol.state.lock().map_err(epoch_poison)?;
        if state.state != QueueEpochState::Open {
            return Err(protocol_error("DVRキューは排出中または閉鎖済みです"));
        }
        state.admitted_transaction_count = state
            .admitted_transaction_count
            .checked_add(1)
            .ok_or_else(|| protocol_error("DVRキューのトランザクション数が上限を超えました"))?;
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
            .ok_or_else(|| protocol_error("DVRキューのエポック制御が設定されていません"))?;
        let mut state = protocol.state.lock().map_err(epoch_poison)?;
        if state.state != QueueEpochState::Open {
            return Err(protocol_error("DVRキューのエポックが開いていません"));
        }
        let next_epoch = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| protocol_error("DVRキューのエポックを更新できません"))?;
        state.state = QueueEpochState::Draining;
        while state.admitted_transaction_count != 0 {
            state = protocol
                .state
                .wait(&protocol.drained, state)
                .map_err(epoch_poison)?;
            if state.state != QueueEpochState::Draining {
                return Err(protocol_error("待機中にDVRキューが排出状態を離れました"));
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
            .ok_or_else(|| protocol_error("DVRキューは再生用ではありません"))?;
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVRキューのエポック制御が設定されていません"))?;
        let state = protocol.state.lock().map_err(epoch_poison)?;
        Ok((queue_identity, state.epoch))
    }

    pub(crate) fn close_dvr_protocol(&self) -> Result<(), QueueRuntimeError> {
        let protocol = self
            .dvr_epoch
            .as_ref()
            .ok_or_else(|| protocol_error("DVRキューのエポック制御が設定されていません"))?;
        let mut state = protocol.state.lock().map_err(epoch_poison)?;
        state.state = QueueEpochState::Closed;
        protocol.drained.notify_all();
        Ok(())
    }

    pub fn available_to_read(&self) -> Result<usize, QueueRuntimeError> {
        self.queue
            .available_to_read_result()
            .map_err(|err| map_data_path_error(err, "FMQのavailable_to_readに失敗しました"))
    }

    pub fn available_to_write(&self) -> Result<usize, QueueRuntimeError> {
        self.queue
            .available_to_write_result()
            .map_err(|err| map_data_path_error(err, "FMQのavailable_to_writeに失敗しました"))
    }

    pub(crate) fn availability_snapshot(
        &self,
    ) -> Result<QueueAvailabilitySnapshot, QueueRuntimeError> {
        let readable_bytes = self
            .queue
            .current_fill()
            .map_err(|err| map_data_path_error(err, "FMQの充填量取得に失敗しました"))?;
        let writable_bytes = self
            .capacity_bytes
            .checked_sub(readable_bytes)
            .ok_or_else(|| protocol_error("FMQの充填量が設定済みキュー容量を超えています"))?;
        Ok(QueueAvailabilitySnapshot {
            readable_bytes,
            writable_bytes,
        })
    }

    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, QueueRuntimeError> {
        self.queue
            .read_into(data)
            .map_err(|err| map_data_path_error(err, "FMQの読み取りに失敗しました"))
    }

    pub fn write_checked(&self, data: &[u8]) -> Result<usize, QueueRuntimeError> {
        self.queue
            .write_checked(data)
            .map_err(|err| map_data_path_error(err, "FMQの書き込みに失敗しました"))
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), QueueRuntimeError> {
        let result = self
            .queue
            .wake(event_mask)
            .map_err(|err| map_data_path_error(err, "FMQの起床通知に失敗しました"));
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
        .map_err(|err| map_export_error(err, "FMQ grantor数の出力に失敗しました"))?;
    let mut grantors = Vec::with_capacity(grantor_count);
    for index in 0..grantor_count {
        let (fd_index, offset, extent) = queue
            .grantor_at_result(index)
            .map_err(|err| map_export_error(err, "FMQ grantorの出力に失敗しました"))?;
        grantors.push(QueueGrantorDescriptorSnapshot {
            fd_index,
            offset,
            extent,
        });
    }

    let fd_count = queue
        .fd_count_result()
        .map_err(|err| map_export_error(err, "FMQ fd数の出力に失敗しました"))?;
    let mut fds = Vec::with_capacity(fd_count);
    let mut fd_sizes = Vec::with_capacity(fd_count);
    for index in 0..fd_count {
        let fd = queue
            .dup_fd_at_result(index)
            .map_err(|err| map_export_error(err, "FMQ fdの出力に失敗しました"))?;
        let file = unsafe { File::from_raw_fd(fd) };
        let fd_size_u64 = file
            .metadata()
            .map_err(|_| {
                QueueRuntimeError::new(
                    QueueRuntimeErrorKind::StructuralDescriptor,
                    "FMQ descriptor fdのメタデータ取得に失敗しました",
                )
            })?
            .len();
        let fd_size = i64::try_from(fd_size_u64).map_err(|_| {
            QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptor fdのサイズが上限を超えました",
            )
        })?;
        fd_sizes.push(fd_size);
        fds.push(file);
    }

    let int_count = queue
        .int_count_result()
        .map_err(|err| map_export_error(err, "FMQ int数の出力に失敗しました"))?;
    if int_count > 4 {
        return Err(QueueRuntimeError::new(
            QueueRuntimeErrorKind::StructuralDescriptor,
            "FMQ descriptorのint数が不正です",
        ));
    }
    let mut ints = Vec::with_capacity(int_count);
    for index in 0..int_count {
        ints.push(
            queue
                .int_at_result(index)
                .map_err(|err| map_export_error(err, "FMQ intの出力に失敗しました"))?,
        );
    }

    validate_grantor_ranges_against_fd_sizes(&grantors, &fd_sizes)?;

    let quantum = queue
        .quantum_result()
        .map_err(|err| map_export_error(err, "FMQ quantumの出力に失敗しました"))?;
    if quantum <= 0 {
        return Err(QueueRuntimeError::new(
            QueueRuntimeErrorKind::StructuralDescriptor,
            "FMQ descriptorのquantumが不正です",
        ));
    }
    let flags = queue
        .flags_result()
        .map_err(|err| map_export_error(err, "FMQ flagsの出力に失敗しました"))?;

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
                "FMQ descriptorのgrantor fd indexが範囲外です",
            ));
        }
        if grantor.offset < 0 || grantor.extent <= 0 {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptorのgrantor範囲が不正です",
            ));
        }
        let Some(end) = i64::from(grantor.offset).checked_add(grantor.extent) else {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptorのgrantor範囲が上限を超えました",
            ));
        };
        let fd_size = fd_sizes[grantor.fd_index as usize];
        if fd_size > 0 && end > fd_size {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::StructuralDescriptor,
                "FMQ descriptorのgrantor範囲がfdサイズを超えています",
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
    // 下位2ビットは局所取消しの種別、bit 2はdataロック汚染、上位ビットは検出回数。
    cleanup_failures: AtomicU64,
}

impl GateInner {
    fn check_cleanup(&self) -> Result<(), QueueRuntimeError> {
        let failures = self.cleanup_failures.load(Ordering::Acquire);
        if failures & 4 != 0 {
            Err(Self::poison_error(failures))
        } else if failures == 0 {
            Ok(())
        } else {
            Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::GateCleanupFailed {
                    producer_release: failures & 1 != 0,
                    drain_rollback: failures & 2 != 0,
                },
                "filter gateの局所後片付けに失敗しました",
            ))
        }
    }

    fn poison_error(failures: u64) -> QueueRuntimeError {
        QueueRuntimeError::new(
            QueueRuntimeErrorKind::GateLockPoisoned {
                lock: QueueRuntimeLockKind::FilterProducerDrainGateData,
                poison_count: failures >> 3,
                counter_saturated: failures >> 3 == u64::MAX >> 3,
                producer_release: failures & 1 != 0,
                drain_rollback: failures & 2 != 0,
            },
            "filter gateデータロックが汚染されています",
        )
    }

    fn data_lock_poison(&self) -> QueueRuntimeError {
        self.record_cleanup_poison(0);
        Self::poison_error(self.cleanup_failures.load(Ordering::Acquire))
    }

    fn lock_data(&self) -> Result<std::sync::MutexGuard<'_, GateData>, QueueRuntimeError> {
        self.data.lock().map_err(|_| self.data_lock_poison())
    }

    fn record_cleanup_failure(&self, failure: u64) {
        self.cleanup_failures.fetch_or(failure, Ordering::Release);
        self.drained.notify_all();
    }

    fn record_cleanup_poison(&self, failure: u64) {
        let mut current = self.cleanup_failures.load(Ordering::Acquire);
        let recorded = loop {
            let count = ((current >> 3) + 1).min(u64::MAX >> 3);
            let next = (count << 3) | (current & 7) | 4 | failure;
            match self.cleanup_failures.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break next,
                Err(observed) => current = observed,
            }
        };
        self.drained.notify_all();
        eprintln!(
            "filter gateロック汚染: ロック=FilterProducerDrainGate.data 検出回数={} 飽和={} producer解放={} drain巻戻し={}",
            recorded >> 3,
            recorded >> 3 == u64::MAX >> 3,
            recorded & 1 != 0,
            recorded & 2 != 0,
        );
    }
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
#[must_use = "解放失敗を観測できるよう、producer permitを明示的に確定してください"]
pub(crate) struct FilterProducerPermit {
    inner: Arc<GateInner>,
    delivery_generation: u64,
    active: bool,
}

#[derive(Debug)]
#[must_use = "巻き戻し失敗を観測できるよう、排出トランザクションを明示的に確定してください"]
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
                "filter producer gateの保留イベント容量は正数である必要があります",
            ));
        }
        let mut pending_events = VecDeque::new();
        pending_events
            .try_reserve_exact(pending_event_capacity)
            .map_err(|_| gate_error("filter producer gateの保留イベント領域を確保できません"))?;
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
                cleanup_failures: AtomicU64::new(0),
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn poison_data_lock_for_test(&self) {
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.inner.data.lock().unwrap();
            panic!("filter gateデータを汚染");
        }))
        .is_err());
    }

    pub(crate) fn begin_producer(&self) -> Result<FilterProducerPermit, QueueRuntimeError> {
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        if data.state != GateState::Open {
            return Err(gate_error("filter producer gateは排出中または閉鎖済みです"));
        }
        data.admitted_producer_count = data
            .admitted_producer_count
            .checked_add(1)
            .ok_or_else(|| gate_error("filter producer permit数が上限を超えました"))?;
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
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        if data.state != GateState::Open {
            return Err(gate_error("filter producer gateが開いていません"));
        }
        let next_parser_generation = data
            .parser_state_generation
            .checked_add(1)
            .ok_or_else(|| gate_error("filter parserの世代を発行できません"))?;
        let next_delivery_generation = match boundary {
            FilterDrainBoundary::Flush => data.filter_delivery_generation,
            FilterDrainBoundary::Reconfigure => data
                .filter_delivery_generation
                .checked_add(1)
                .ok_or_else(|| gate_error("filter配送の世代を発行できません"))?,
        };
        data.state = GateState::Draining;
        while data.admitted_producer_count != 0 {
            data = self
                .inner
                .drained
                .wait(data)
                .map_err(|_| self.inner.data_lock_poison())?;
            if data.state != GateState::Draining {
                return Err(gate_error(
                    "待機中にfilter producer gateが排出状態を離れました",
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
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        match data.state {
            GateState::Open => Ok(data.pending_events.drain(..).collect()),
            GateState::Draining => Ok(Vec::new()),
            GateState::Closed => Err(gate_error(
                "イベント取得中にfilter producer gateが閉じられました",
            )),
        }
    }

    pub(crate) fn close(&self) -> Result<(), QueueRuntimeError> {
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        data.state = GateState::Closed;
        data.pending_events.clear();
        self.inner.drained.notify_all();
        Ok(())
    }
}

impl FilterProducerPermit {
    pub(crate) fn record_output_byte_offset(&self) -> Result<u64, QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if !self.active {
            return Err(gate_error("filter producer permitは既に消費されています"));
        }
        let data = self.inner.lock_data()?;
        if data.state == GateState::Closed
            || data.filter_delivery_generation != self.delivery_generation
        {
            return Err(gate_error("filter producer permitは失効しています"));
        }
        Ok(data.record_output_byte_offset)
    }

    pub(crate) fn commit_record_output(
        mut self,
        committed_bytes: usize,
        event: Option<PipelineGeneratedEvent>,
    ) -> Result<(), QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if !self.active || committed_bytes == 0 {
            return Err(gate_error("録画出力の確定内容が不正です"));
        }
        let committed_bytes = u64::try_from(committed_bytes)
            .map_err(|_| gate_error("録画出力のバイト数が範囲外です"))?;
        let mut data = self.inner.lock_data()?;
        if data.filter_delivery_generation != self.delivery_generation {
            return Err(gate_error("filter producer permitの世代が変化しました"));
        }
        let next_offset = data
            .record_output_byte_offset
            .checked_add(committed_bytes)
            .ok_or_else(|| gate_error("録画出力のバイト位置を更新できません"))?;
        let event_queue_full =
            event.is_some() && data.pending_events.len() >= data.pending_event_capacity;
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
            .ok_or_else(|| gate_error("filter producer permit数が下限を下回りました"))?;
        self.active = false;
        if data.admitted_producer_count == 0 {
            self.inner.drained.notify_all();
        }
        if event_queue_full {
            Err(gate_error("filter producerの保留イベントキューが満杯です"))
        } else if gate_state == GateState::Closed {
            Err(gate_error(
                "録画出力の確定前にfilter producer gateが閉じられました",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn enqueue_event(
        &mut self,
        event: PipelineGeneratedEvent,
    ) -> Result<(), QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if !self.active {
            return Err(gate_error("filter producer permitは既に消費されています"));
        }
        let mut data = self.inner.lock_data()?;
        if data.state == GateState::Closed
            || data.filter_delivery_generation != self.delivery_generation
        {
            return Err(gate_error("filter producer permitは失効しています"));
        }
        if data.pending_events.len() >= data.pending_event_capacity {
            return Err(gate_error("filter producerの保留イベントキューが満杯です"));
        }
        data.pending_events.push_back(event);
        Ok(())
    }

    fn release(&mut self) -> Result<GateState, QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if !self.active {
            return Err(gate_error("filter producer permitは既に消費されています"));
        }
        let mut data = self.inner.lock_data()?;
        if data.filter_delivery_generation != self.delivery_generation {
            return Err(gate_error("filter producer permitの世代が変化しました"));
        }
        let gate_state = data.state;
        data.admitted_producer_count = data
            .admitted_producer_count
            .checked_sub(1)
            .ok_or_else(|| gate_error("filter producer permit数が下限を下回りました"))?;
        self.active = false;
        if data.admitted_producer_count == 0 {
            self.inner.drained.notify_all();
        }
        Ok(gate_state)
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if self.release()? == GateState::Closed {
            Err(gate_error("確定前にfilter producer gateが閉じられました"))
        } else {
            Ok(())
        }
    }
}

impl Drop for FilterProducerPermit {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // 局所的な許可証返却だけを行い、汚染時に再lockして診断を失わない。
        match self.inner.data.lock() {
            Ok(mut data) => {
                if data.filter_delivery_generation == self.delivery_generation
                    && data.admitted_producer_count != 0
                {
                    data.admitted_producer_count -= 1;
                    self.inner.drained.notify_all();
                } else {
                    data.state = GateState::Closed;
                    data.pending_events.clear();
                    self.inner.record_cleanup_failure(1);
                }
            }
            Err(_) => self.inner.record_cleanup_poison(1),
        }
        self.active = false;
    }
}

impl FilterDrainTxn {
    pub(crate) fn take_pending_events(
        &mut self,
    ) -> Result<Vec<PipelineGeneratedEvent>, QueueRuntimeError> {
        self.inner.check_cleanup()?;
        if !self.active {
            return Err(gate_error(
                "filter producerの排出権限は既に消費されています",
            ));
        }
        let mut data = self.inner.lock_data()?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error(
                "イベント排出前にfilter producerの状態が変化しました",
            ));
        }
        Ok(data.pending_events.drain(..).collect())
    }

    pub(crate) fn commit(mut self) -> Result<(), QueueRuntimeError> {
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error(
                "確定前にfilter producerの排出状態が変化しました",
            ));
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
        self.inner.check_cleanup()?;
        let mut data = self.inner.lock_data()?;
        if data.state != GateState::Draining
            || data.filter_delivery_generation != self.delivery_generation
            || data.parser_state_generation != self.parser_generation
            || data.admitted_producer_count != 0
        {
            return Err(gate_error(
                "確定前にfilter producerの排出状態が変化しました",
            ));
        }
        let pending_events = data.pending_events.drain(..).collect();
        data.filter_delivery_generation = self.next_delivery_generation;
        data.parser_state_generation = self.next_parser_generation;
        data.state = GateState::Open;
        self.active = false;
        self.inner.drained.notify_all();
        Ok(pending_events)
    }
}

impl Drop for FilterDrainTxn {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // 外部I/Oを行わず、未確定の局所drain予約だけを取り消す。
        match self.inner.data.lock() {
            Ok(mut data) => {
                if data.state == GateState::Draining
                    && data.filter_delivery_generation == self.delivery_generation
                    && data.parser_state_generation == self.parser_generation
                {
                    data.state = GateState::Open;
                    self.inner.drained.notify_all();
                } else {
                    data.state = GateState::Closed;
                    data.pending_events.clear();
                    self.inner.record_cleanup_failure(2);
                }
            }
            Err(_) => self.inner.record_cleanup_poison(2),
        }
        self.active = false;
    }
}

#[cfg(test)]
mod dvr_queue_cleanup_tests {
    use super::*;

    #[test]
    fn abandoned_filter_permit_and_drain_cancel_only_local_reservations() {
        let gate = FilterProducerDrainGate::new(4).unwrap();
        drop(gate.begin_producer().unwrap());
        drop(gate.begin_drain(FilterDrainBoundary::Reconfigure).unwrap());
        let permit = gate.begin_producer().unwrap();
        assert_eq!(permit.delivery_generation, 0);
        permit.commit().unwrap();
        let data = gate.inner.data.lock().unwrap();
        assert_eq!(data.admitted_producer_count, 0);
        assert_eq!(data.parser_state_generation, 0);
        assert_eq!(data.state, GateState::Open);
    }

    #[test]
    fn poisoned_filter_cleanup_is_retained_and_blocks_later_operations() {
        for producer_release in [true, false] {
            let gate = FilterProducerDrainGate::new(4).unwrap();
            let permit = producer_release.then(|| gate.begin_producer().unwrap());
            let drain =
                (!producer_release).then(|| gate.begin_drain(FilterDrainBoundary::Flush).unwrap());
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = gate.inner.data.lock().unwrap();
                panic!("filter gateを汚染");
            }))
            .is_err());
            drop(permit);
            drop(drain);
            let expected = QueueRuntimeErrorKind::GateLockPoisoned {
                lock: QueueRuntimeLockKind::FilterProducerDrainGateData,
                poison_count: 1,
                counter_saturated: false,
                producer_release,
                drain_rollback: !producer_release,
            };
            assert_eq!(gate.begin_producer().unwrap_err().kind, expected);
            assert_eq!(
                gate.begin_drain(FilterDrainBoundary::Flush)
                    .unwrap_err()
                    .kind,
                expected
            );
            assert!(gate.inner.data.is_poisoned());
        }
    }

    #[test]
    fn filter_cleanup_invariant_failure_is_not_lock_poison() {
        let gate = FilterProducerDrainGate::new(4).unwrap();
        let permit = gate.begin_producer().unwrap();
        gate.inner.data.lock().unwrap().admitted_producer_count = 0;
        drop(permit);
        assert_eq!(
            gate.begin_producer().unwrap_err().kind,
            QueueRuntimeErrorKind::GateCleanupFailed {
                producer_release: true,
                drain_rollback: false,
            }
        );
        assert!(!gate.inner.data.is_poisoned());
    }

    #[test]
    fn filter_cleanup_poison_count_survives_multiple_drops_and_saturates() {
        let gate = FilterProducerDrainGate::new(4).unwrap();
        let first = gate.begin_producer().unwrap();
        let second = gate.begin_producer().unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = gate.inner.data.lock().unwrap();
            panic!("filter gateを汚染");
        }))
        .is_err());
        drop(first);
        drop(second);
        assert!(matches!(
            gate.inner.check_cleanup().unwrap_err().kind,
            QueueRuntimeErrorKind::GateLockPoisoned {
                poison_count: 2,
                ..
            }
        ));
        gate.inner
            .cleanup_failures
            .store(u64::MAX - 2, Ordering::Release);
        gate.inner.record_cleanup_poison(2);
        assert_eq!(
            gate.inner.check_cleanup().unwrap_err().kind,
            QueueRuntimeErrorKind::GateLockPoisoned {
                lock: QueueRuntimeLockKind::FilterProducerDrainGateData,
                poison_count: u64::MAX >> 3,
                counter_saturated: true,
                producer_release: true,
                drain_rollback: true,
            }
        );
    }

    #[test]
    fn epoch_poison_and_abort_poison_survive_drain_failure() {
        let queue = QueueRuntime::new_dvr(64, false, true).unwrap();
        let drain = queue.begin_dvr_drain().unwrap();
        let protocol = queue.dvr_epoch.as_ref().unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = protocol.state.lock().unwrap();
            panic!("汚染を注入");
        }));
        let error = queue
            .commit_dvr_drain_with_queue_clear_operation(drain, |_| panic!("汚染後の消去は禁止"))
            .unwrap_err();
        assert_eq!(error.step, DvrQueueDrainStep::EpochCommit);
        let QueueRuntimeErrorKind::EpochLockPoisoned(primary) = error.primary.kind else {
            panic!("主障害の分類が消失");
        };
        let QueueRuntimeErrorKind::EpochLockPoisoned(rollback) = error.rollback.unwrap().kind
        else {
            panic!("取消し障害の分類が消失");
        };
        assert_eq!(
            primary.lock,
            RuntimeLockKind::DvrQueueEpoch {
                queue_identity: protocol.queue_identity
            }
        );
        assert_eq!(primary.poison_count, 1);
        assert_eq!(rollback.poison_count, 2);
        assert!(matches!(
            queue.begin_dvr_read(1),
            Err(QueueRuntimeError {
                kind: QueueRuntimeErrorKind::EpochLockPoisoned(_),
                ..
            })
        ));
        assert!(matches!(
            queue.close_dvr_protocol(),
            Err(QueueRuntimeError {
                kind: QueueRuntimeErrorKind::EpochLockPoisoned(_),
                ..
            })
        ));
    }

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

        assert_eq!(error.step, DvrQueueDrainStep::QueueClear);
        assert!(error.rollback.is_none());
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

        assert_eq!(error.step, DvrQueueDrainStep::EpochCommit);
        assert!(error.rollback.is_none());
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

#[cfg(test)]
mod queue_epoch_authority_drop_contract_tests {
    use super::*;

    fn protocol(state: QueueEpochState, epoch: u64, admitted: usize) -> Arc<QueueEpochProtocol> {
        Arc::new(QueueEpochProtocol {
            state: PoisonTrackedMutex::new(
                QueueEpochProtocolState {
                    state,
                    epoch,
                    admitted_transaction_count: admitted,
                },
                RuntimeLockKind::DvrQueueEpoch {
                    queue_identity: Some(77),
                },
            ),
            drained: Condvar::new(),
            queue_identity: Some(77),
        })
    }

    #[test]
    fn dropping_unconsumed_queue_epoch_authority_fail_closes_without_rollback() {
        let protocol = protocol(QueueEpochState::Open, 4, 1);
        let token = QueueEpochToken {
            protocol: Arc::clone(&protocol),
            queue_identity: Some(77),
            epoch: 4,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        };

        drop(token);

        let state = protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Closed);
        assert_eq!(state.epoch, 4);
        assert_eq!(state.admitted_transaction_count, 1);
    }

    #[test]
    fn dropping_unconsumed_queue_drain_authority_fail_closes_without_rollback() {
        let protocol = protocol(QueueEpochState::Draining, 9, 0);
        let txn = QueueEpochDrainTxn {
            protocol: Arc::clone(&protocol),
            epoch: 9,
            next_epoch: 10,
            active: true,
        };

        drop(txn);

        let state = protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Closed);
        assert_eq!(state.epoch, 9);
    }

    #[test]
    fn explicit_queue_authority_abort_is_the_only_normal_rollback_path() {
        let transaction_protocol = protocol(QueueEpochState::Open, 11, 1);
        QueueEpochToken {
            protocol: Arc::clone(&transaction_protocol),
            queue_identity: Some(77),
            epoch: 11,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        }
        .abort()
        .unwrap();
        {
            let state = transaction_protocol.state.lock().unwrap();
            assert_eq!(state.state, QueueEpochState::Open);
            assert_eq!(state.admitted_transaction_count, 0);
        }

        let drain_protocol = protocol(QueueEpochState::Draining, 12, 0);
        QueueEpochDrainTxn {
            protocol: Arc::clone(&drain_protocol),
            epoch: 12,
            next_epoch: 13,
            active: true,
        }
        .abort()
        .unwrap();
        let state = drain_protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Open);
        assert_eq!(state.epoch, 12);
    }

    #[test]
    fn fail_closed_drop_preserves_poison_and_rejects_later_commits() {
        let protocol = protocol(QueueEpochState::Open, 13, 1);
        let poison_target = Arc::clone(&protocol);
        assert!(std::thread::spawn(move || {
            let _guard = poison_target.state.lock().unwrap();
            panic!("poison queue epoch lock for drop-backstop test");
        })
        .join()
        .is_err());

        let token = QueueEpochToken {
            protocol: Arc::clone(&protocol),
            queue_identity: Some(77),
            epoch: 13,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        };
        drop(token);

        assert!(protocol.state.lock().is_err());
        let result = QueueEpochToken {
            protocol: Arc::clone(&protocol),
            queue_identity: Some(77),
            epoch: 13,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        }
        .commit();
        assert!(matches!(
            result.unwrap_err().kind,
            QueueRuntimeErrorKind::EpochLockPoisoned(_)
        ));
        assert!(protocol.state.lock().is_err());
    }
}
