use std::fmt;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::sync::Arc;

use maleicacid_tuner_hal2_fmq::{FmqQueue, FmqQueueError};

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

impl QueueRuntimeError {
    const fn new(kind: QueueRuntimeErrorKind, detail: &'static str) -> Self {
        Self { kind, detail }
    }
}

pub struct QueueRuntime {
    queue: Arc<FmqQueue>,
    capacity_bytes: usize,
    configure_event_flag: bool,
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
    pub fn new(buffer_size: i32, configure_event_flag: bool) -> Result<Self, QueueRuntimeError> {
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
        Ok(Self {
            queue: Arc::new(queue),
            capacity_bytes,
            configure_event_flag,
        })
    }

    pub(crate) fn capacity_matches_buffer_size(&self, buffer_size: i32) -> bool {
        usize::try_from(buffer_size).ok() == Some(self.capacity_bytes)
    }

    pub fn clear(&mut self) -> Result<(), QueueRuntimeError> {
        self.queue
            .clear()
            .map_err(|err| map_data_path_error(err, "FMQ clear failed"))
    }

    #[cfg(test)]
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

    #[cfg(test)]
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
        self.queue
            .wake(event_mask)
            .map_err(|err| map_data_path_error(err, "FMQ wake failed"))
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
        FmqQueueError::NativeReadZero
        | FmqQueueError::NativeWriteFailed
        | FmqQueueError::NativeWriteInvalidArgument
        | FmqQueueError::NativeWakeFailed => QueueRuntimeErrorKind::DataPathFailure,
        _ => QueueRuntimeErrorKind::StructuralDescriptor,
    };
    QueueRuntimeError::new(kind, detail)
}
