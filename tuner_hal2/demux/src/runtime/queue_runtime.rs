use std::fmt;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicU64, Ordering};

use maleicacid_tuner_hal2_fmq::{
    FmqDescriptorHandle, FmqQueueError, FmqReader, FmqWaitOutcome, FmqWaiter, FmqWriter,
};

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
    RoleViolation,
    ExportTransient,
    DataPathFailure,
    StructuralDescriptor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueRuntimeError {
    pub kind: QueueRuntimeErrorKind,
    pub detail: &'static str,
}

impl QueueRuntimeError {
    const fn new(kind: QueueRuntimeErrorKind, detail: &'static str) -> Self {
        Self { kind, detail }
    }
}

static NEXT_QUEUE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn next_queue_instance_id() -> Result<u64, QueueRuntimeError> {
    NEXT_QUEUE_INSTANCE_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1))
        .map_err(|_| QueueRuntimeError::new(
            QueueRuntimeErrorKind::NativeCreateFailed,
            "FMQ queue instance id exhausted",
        ))
}

enum QueueEndpoint {
    HalWriter(FmqWriter),
    HalReader(FmqReader),
}

pub struct QueueRuntime {
    instance_id: u64,
    endpoint: QueueEndpoint,
    capacity_bytes: usize,
    configure_event_flag: bool,
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueueRuntimeRollbackState {
    pub instance_id: u64,
    pub current_fill: usize,
}

pub(crate) struct QueueDescriptorExportHandle {
    handle: FmqDescriptorHandle,
}

/// playback worker 用に別 map した EventFlag endpoint。wait 中に HAL-reader endpoint の
/// mutex を保持しない。
pub struct QueueWaitHandle {
    waiter: FmqWaiter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueWaitResult {
    Signaled(u32),
    TimedOut,
}

impl QueueWaitHandle {
    pub fn wait(
        &self,
        event_mask: u32,
        timeout_ns: i64,
    ) -> Result<QueueWaitResult, QueueRuntimeError> {
        match self.waiter.wait(event_mask, timeout_ns) {
            Ok(FmqWaitOutcome::Signaled(state)) => Ok(QueueWaitResult::Signaled(state)),
            Ok(FmqWaitOutcome::TimedOut) => Ok(QueueWaitResult::TimedOut),
            Err(err) => Err(map_data_path_error(err, "FMQ EventFlag wait failed")),
        }
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), QueueRuntimeError> {
        self.waiter
            .wake(event_mask)
            .map_err(|err| map_data_path_error(err, "FMQ EventFlag wake failed"))
    }
}

impl fmt::Debug for QueueDescriptorExportHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueDescriptorExportHandle").finish()
    }
}

impl QueueDescriptorExportHandle {
    fn export_descriptor(&self) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
        export_queue_descriptor(&self.handle)
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
        let role = match &self.endpoint {
            QueueEndpoint::HalWriter(_) => "hal-writer",
            QueueEndpoint::HalReader(_) => "hal-reader",
        };
        f.debug_struct("QueueRuntime")
            .field("instance_id", &self.instance_id)
            .field("role", &role)
            .field("capacity_bytes", &self.capacity_bytes)
            .field("configure_event_flag", &self.configure_event_flag)
            .finish()
    }
}

impl QueueRuntime {
    fn checked_capacity(buffer_size: i32) -> Result<usize, QueueRuntimeError> {
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
        Ok(capacity_bytes)
    }

    pub(crate) fn new_writer(
        buffer_size: i32,
        configure_event_flag: bool,
    ) -> Result<Self, QueueRuntimeError> {
        let capacity_bytes = Self::checked_capacity(buffer_size)?;
        let writer = FmqWriter::create(capacity_bytes, configure_event_flag)
            .map_err(|err| map_create_error(err, "FMQ writer create failed"))?;
        Ok(Self {
            instance_id: next_queue_instance_id()?,
            endpoint: QueueEndpoint::HalWriter(writer),
            capacity_bytes,
            configure_event_flag,
        })
    }

    pub(crate) fn new_reader(
        buffer_size: i32,
        configure_event_flag: bool,
    ) -> Result<Self, QueueRuntimeError> {
        let capacity_bytes = Self::checked_capacity(buffer_size)?;
        let reader = FmqReader::create(capacity_bytes, configure_event_flag)
            .map_err(|err| map_create_error(err, "FMQ reader create failed"))?;
        Ok(Self {
            instance_id: next_queue_instance_id()?,
            endpoint: QueueEndpoint::HalReader(reader),
            capacity_bytes,
            configure_event_flag,
        })
    }

    pub(crate) const fn instance_id(&self) -> u64 {
        self.instance_id
    }

    pub(crate) fn capacity_matches_buffer_size(&self, buffer_size: i32) -> bool {
        usize::try_from(buffer_size).ok() == Some(self.capacity_bytes)
    }

    pub(crate) const fn is_hal_writer(&self) -> bool {
        matches!(&self.endpoint, QueueEndpoint::HalWriter(_))
    }

    pub(crate) const fn is_hal_reader(&self) -> bool {
        matches!(&self.endpoint, QueueEndpoint::HalReader(_))
    }

    pub(crate) const fn rollback_identity(&self) -> u64 {
        self.instance_id
    }

    pub(crate) fn current_fill(&self) -> Result<usize, QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer.current_fill(),
            QueueEndpoint::HalReader(reader) => reader.current_fill(),
        }
        .map_err(|err| map_data_path_error(err, "FMQ fill query failed"))
    }


    pub(crate) fn rollback_state(&self) -> Result<QueueRuntimeRollbackState, QueueRuntimeError> {
        Ok(QueueRuntimeRollbackState {
            instance_id: self.instance_id,
            current_fill: self.current_fill()?,
        })
    }

    pub(crate) fn available_to_read(&self) -> Result<usize, QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalReader(reader) => reader
                .available_to_read()
                .map_err(|err| map_data_path_error(err, "FMQ available_to_read failed")),
            QueueEndpoint::HalWriter(_) => Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "HAL writer endpoint cannot read FMQ data",
            )),
        }
    }

    pub fn available_to_write(&self) -> Result<usize, QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer
                .available_to_write()
                .map_err(|err| map_data_path_error(err, "FMQ available_to_write failed")),
            QueueEndpoint::HalReader(_) => Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "HAL reader endpoint cannot write FMQ data",
            )),
        }
    }

    pub(crate) fn read_into(&self, data: &mut [u8]) -> Result<usize, QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalReader(reader) => reader
                .read_into(data)
                .map_err(|err| map_data_path_error(err, "FMQ read failed")),
            QueueEndpoint::HalWriter(_) => Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "HAL writer endpoint cannot read FMQ data",
            )),
        }
    }

    pub fn write_checked(&self, data: &[u8]) -> Result<usize, QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer
                .write_checked(data)
                .map_err(|err| map_data_path_error(err, "FMQ write failed")),
            QueueEndpoint::HalReader(_) => Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "HAL reader endpoint cannot write FMQ data",
            )),
        }
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), QueueRuntimeError> {
        match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer.wake(event_mask),
            QueueEndpoint::HalReader(reader) => reader.wake(event_mask),
        }
        .map_err(|err| map_data_path_error(err, "FMQ wake failed"))
    }

    pub(crate) fn wait_handle(&self) -> Result<QueueWaitHandle, QueueRuntimeError> {
        let descriptor = match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer.descriptor_handle(),
            QueueEndpoint::HalReader(reader) => reader.descriptor_handle(),
        };
        let waiter = descriptor
            .open_waiter()
            .map_err(|err| map_data_path_error(err, "FMQ waiter mapping failed"))?;
        Ok(QueueWaitHandle { waiter })
    }

    #[cfg(test)]
    pub(crate) fn peer_read_for_test(
        &self,
        data: &mut [u8],
    ) -> Result<usize, QueueRuntimeError> {
        let QueueEndpoint::HalWriter(writer) = &self.endpoint else {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "framework reader peer requires a HAL writer queue",
            ));
        };
        writer
            .descriptor_handle()
            .open_peer_reader()
            .and_then(|reader| reader.read_into(data))
            .map_err(|err| map_data_path_error(err, "FMQ framework peer read failed"))
    }

    #[cfg(test)]
    pub(crate) fn peer_write_for_test(&self, data: &[u8]) -> Result<usize, QueueRuntimeError> {
        let QueueEndpoint::HalReader(reader) = &self.endpoint else {
            return Err(QueueRuntimeError::new(
                QueueRuntimeErrorKind::RoleViolation,
                "framework writer peer requires a HAL reader queue",
            ));
        };
        reader
            .descriptor_handle()
            .open_peer_writer()
            .and_then(|writer| writer.write_checked(data))
            .map_err(|err| map_data_path_error(err, "FMQ framework peer write failed"))
    }

    pub(crate) fn descriptor_export_handle(&self) -> QueueDescriptorExportHandle {
        let handle = match &self.endpoint {
            QueueEndpoint::HalWriter(writer) => writer.descriptor_handle(),
            QueueEndpoint::HalReader(reader) => reader.descriptor_handle(),
        };
        QueueDescriptorExportHandle { handle }
    }
}

fn export_queue_descriptor(
    queue: &FmqDescriptorHandle,
) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
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
        // Some Android FMQ/ashmem backed fds report metadata len as 0 even though
        // the native FMQ descriptor carries the valid grantor extent.  Treat a
        // zero reported fd size as unknown and keep the structural range checks
        // above; when the platform reports a positive size, keep the strict
        // grantor-vs-fd bound check.
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
        FmqQueueError::NativeReadFailed
        | FmqQueueError::NativeReadInvalidArgument
        | FmqQueueError::NativeResetFailed
        | FmqQueueError::NativeLockPoisoned
        | FmqQueueError::NativeWriteFailed
        | FmqQueueError::NativeWriteInvalidArgument
        | FmqQueueError::NativeWakeFailed => QueueRuntimeErrorKind::DataPathFailure,
        _ => QueueRuntimeErrorKind::StructuralDescriptor,
    };
    QueueRuntimeError::new(kind, detail)
}
