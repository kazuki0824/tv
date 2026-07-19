//! HAL2 FMQ native shim wrapper。
//!
//! binder_service制御層は持たない。ここではlibfmq native shimへの最小Rust接続だけを保持し、配送commitやlifecycleはcontrol層で実装する。

use std::ffi::c_void;
use std::sync::{Arc, Mutex, MutexGuard};

#[repr(C)]
pub struct TunerFmqQueue(c_void);

extern "C" {
    #[link_name = "tuner_fmq_queue_create"]
    fn native_queue_create(num_bytes: usize, configure_event_flag: bool) -> *mut TunerFmqQueue;
    #[link_name = "tuner_fmq_queue_clone"]
    fn native_queue_clone(source: *const TunerFmqQueue) -> *mut TunerFmqQueue;
    #[link_name = "tuner_fmq_queue_destroy"]
    fn native_queue_destroy(queue: *mut TunerFmqQueue);
    #[link_name = "tuner_fmq_queue_available_to_read"]
    fn native_queue_available_to_read(queue: *const TunerFmqQueue) -> usize;
    #[link_name = "tuner_fmq_queue_available_to_write"]
    fn native_queue_available_to_write(queue: *const TunerFmqQueue) -> usize;
    #[link_name = "tuner_fmq_queue_write_checked"]
    fn native_queue_write_checked(
        queue: *mut TunerFmqQueue,
        data: *const u8,
        size: usize,
        out_written: *mut usize,
    ) -> i32;
    #[link_name = "tuner_fmq_queue_read_checked"]
    fn native_queue_read_checked(
        queue: *mut TunerFmqQueue,
        data: *mut u8,
        capacity: usize,
        out_read: *mut usize,
    ) -> i32;
    #[link_name = "tuner_fmq_queue_clear"]
    fn native_queue_clear(queue: *mut TunerFmqQueue, out_discarded: *mut usize) -> i32;
    #[link_name = "tuner_fmq_queue_reset_pointers"]
    fn native_queue_reset_pointers(queue: *mut TunerFmqQueue) -> i32;
    #[link_name = "tuner_fmq_queue_wake"]
    fn native_queue_wake(queue: *mut TunerFmqQueue, bits: u32) -> i32;
    #[link_name = "tuner_fmq_queue_wait"]
    fn native_queue_wait(
        queue: *mut TunerFmqQueue,
        bits: u32,
        timeout_ns: i64,
        state: *mut u32,
    ) -> i32;
    #[link_name = "tuner_fmq_queue_quantum"]
    fn native_queue_quantum(queue: *const TunerFmqQueue) -> i32;
    #[link_name = "tuner_fmq_queue_flags"]
    fn native_queue_flags(queue: *const TunerFmqQueue) -> i32;
    #[link_name = "tuner_fmq_queue_grantor_count"]
    fn native_queue_grantor_count(queue: *const TunerFmqQueue) -> usize;
    #[link_name = "tuner_fmq_queue_grantor_at"]
    fn native_queue_grantor_at(
        queue: *const TunerFmqQueue,
        index: usize,
        fd_index: *mut i32,
        offset: *mut i32,
        extent: *mut i64,
    ) -> bool;
    #[link_name = "tuner_fmq_queue_fd_count"]
    fn native_queue_fd_count(queue: *const TunerFmqQueue) -> usize;
    #[link_name = "tuner_fmq_queue_dup_fd_at"]
    fn native_queue_dup_fd_at(queue: *const TunerFmqQueue, index: usize) -> i32;
    #[link_name = "tuner_fmq_queue_int_count"]
    fn native_queue_int_count(queue: *const TunerFmqQueue) -> usize;
    #[link_name = "tuner_fmq_queue_int_at"]
    fn native_queue_int_at(queue: *const TunerFmqQueue, index: usize, value: *mut i32) -> bool;
}

struct NativeFmqQueue {
    queue: *mut TunerFmqQueue,
}

// Safety: ownership of the opaque native queue is unique and destruction is performed once by
// Drop. Thread sharing is provided only through Mutex<NativeFmqQueue>; NativeFmqQueue itself is
// deliberately not Sync.
unsafe impl Send for NativeFmqQueue {}

impl NativeFmqQueue {
    fn create(num_bytes: usize, configure_event_flag: bool) -> Option<Self> {
        let queue = unsafe { native_queue_create(num_bytes, configure_event_flag) };
        (!queue.is_null()).then_some(Self { queue })
    }

    fn clone_endpoint(&self) -> Option<Self> {
        let queue = unsafe { native_queue_clone(self.queue) };
        (!queue.is_null()).then_some(Self { queue })
    }

    fn available_to_read(&self) -> usize {
        unsafe { native_queue_available_to_read(self.queue) }
    }

    fn available_to_write(&self) -> usize {
        unsafe { native_queue_available_to_write(self.queue) }
    }

    fn write_checked(&mut self, data: &[u8]) -> Result<usize, i32> {
        let mut written = 0usize;
        let (ptr, len) = if data.is_empty() {
            (std::ptr::null(), 0usize)
        } else {
            (data.as_ptr(), data.len())
        };
        let status = unsafe { native_queue_write_checked(self.queue, ptr, len, &mut written) };
        (status == 0).then_some(written).ok_or(status)
    }

    fn read_checked(&mut self, data: &mut [u8]) -> Result<usize, i32> {
        let mut read = 0usize;
        let (ptr, capacity) = if data.is_empty() {
            (std::ptr::null_mut(), 0usize)
        } else {
            (data.as_mut_ptr(), data.len())
        };
        let status = unsafe { native_queue_read_checked(self.queue, ptr, capacity, &mut read) };
        (status == 0).then_some(read).ok_or(status)
    }

    fn clear(&mut self) -> Result<usize, i32> {
        let mut discarded = 0usize;
        let status = unsafe { native_queue_clear(self.queue, &mut discarded) };
        (status == 0).then_some(discarded).ok_or(status)
    }

    fn reset_pointers(&mut self) -> Result<(), i32> {
        let status = unsafe { native_queue_reset_pointers(self.queue) };
        (status == 0).then_some(()).ok_or(status)
    }

    fn wake(&mut self, bits: u32) -> i32 {
        unsafe { native_queue_wake(self.queue, bits) }
    }

    fn wait(&mut self, bits: u32, timeout_ns: i64) -> Result<u32, i32> {
        let mut state = 0u32;
        let status = unsafe { native_queue_wait(self.queue, bits, timeout_ns, &mut state) };
        (status == 0).then_some(state).ok_or(status)
    }

    fn quantum(&self) -> i32 {
        unsafe { native_queue_quantum(self.queue) }
    }

    fn flags(&self) -> i32 {
        unsafe { native_queue_flags(self.queue) }
    }

    fn grantor_count(&self) -> usize {
        unsafe { native_queue_grantor_count(self.queue) }
    }

    fn grantor_at(&self, index: usize) -> Option<(i32, i32, i64)> {
        let (mut fd_index, mut offset, mut extent) = (0i32, 0i32, 0i64);
        let ok = unsafe {
            native_queue_grantor_at(self.queue, index, &mut fd_index, &mut offset, &mut extent)
        };
        ok.then_some((fd_index, offset, extent))
    }

    fn fd_count(&self) -> usize {
        unsafe { native_queue_fd_count(self.queue) }
    }

    fn dup_fd_at(&self, index: usize) -> i32 {
        unsafe { native_queue_dup_fd_at(self.queue, index) }
    }

    fn int_count(&self) -> usize {
        unsafe { native_queue_int_count(self.queue) }
    }

    fn int_at(&self, index: usize) -> Option<i32> {
        let mut value = 0i32;
        unsafe { native_queue_int_at(self.queue, index, &mut value) }.then_some(value)
    }
}

impl Drop for NativeFmqQueue {
    fn drop(&mut self) {
        unsafe { native_queue_destroy(self.queue) };
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqQueueError {
    NativeCreateFailed,
    NativeLockPoisoned,
    NativeWriteInvalidArgument,
    NativeWriteFailed,
    NativeReadInvalidArgument,
    NativeReadFailed,
    NativeWaitFailed,
    NativeWakeFailed,
    DescriptorGrantorUnavailable,
    DescriptorFdDupFailed,
    DescriptorIntUnavailable,
}

struct FmqQueueCore {
    native: Mutex<NativeFmqQueue>,
    capacity_bytes: usize,
}

impl FmqQueueCore {
    fn create(num_bytes: usize, configure_event_flag: bool) -> Result<Arc<Self>, FmqQueueError> {
        let native = NativeFmqQueue::create(num_bytes, configure_event_flag)
            .ok_or(FmqQueueError::NativeCreateFailed)?;
        Ok(Arc::new(Self {
            native: Mutex::new(native),
            capacity_bytes: num_bytes,
        }))
    }

    fn lock(&self) -> Result<MutexGuard<'_, NativeFmqQueue>, FmqQueueError> {
        self.native
            .lock()
            .map_err(|_| FmqQueueError::NativeLockPoisoned)
    }

    fn clone_endpoint(&self) -> Result<Arc<Self>, FmqQueueError> {
        let native = self
            .lock()?
            .clone_endpoint()
            .ok_or(FmqQueueError::NativeCreateFailed)?;
        Ok(Arc::new(Self {
            native: Mutex::new(native),
            capacity_bytes: self.capacity_bytes,
        }))
    }
}

/// HAL-owned writer endpoint. No read API is exposed, preserving FMQ's single-reader contract.
pub struct FmqWriter {
    core: Arc<FmqQueueCore>,
}

/// HAL-owned reader endpoint. No write API is exposed, preserving FMQ's single-writer contract.
pub struct FmqReader {
    core: Arc<FmqQueueCore>,
}

/// export 済み descriptor から作る framework 側 test/adapter writer endpoint。
pub struct FmqPeerWriter {
    core: Arc<FmqQueueCore>,
}

/// export 済み descriptor から作る framework 側 test/adapter reader endpoint。
pub struct FmqPeerReader {
    core: Arc<FmqQueueCore>,
}

/// EventFlag wait endpoint。別 map した libfmq object を所有し、wait 中に HAL reader の
/// native mutex を保持しない。
pub struct FmqWaiter {
    core: Arc<FmqQueueCore>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqWaitOutcome {
    Signaled(u32),
    TimedOut,
}

pub struct FmqDescriptorHandle {
    core: Arc<FmqQueueCore>,
}

impl FmqWriter {
    pub fn create(num_bytes: usize, configure_event_flag: bool) -> Result<Self, FmqQueueError> {
        Ok(Self {
            core: FmqQueueCore::create(num_bytes, configure_event_flag)?,
        })
    }

    pub fn write_checked(&self, bytes: &[u8]) -> Result<usize, FmqQueueError> {
        self.core
            .lock()?
            .write_checked(bytes)
            .map_err(|status| match status {
                -1 => FmqQueueError::NativeWriteInvalidArgument,
                _ => FmqQueueError::NativeWriteFailed,
            })
    }

    pub fn available_to_write(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.available_to_write())
    }

    pub fn current_fill(&self) -> Result<usize, FmqQueueError> {
        let available_to_write = self.core.lock()?.available_to_write();
        Ok(self.core.capacity_bytes.saturating_sub(available_to_write))
    }

    pub fn reset_pointers_for_flush(&self) -> Result<(), FmqQueueError> {
        self.core
            .lock()?
            .reset_pointers()
            .map_err(|_| FmqQueueError::NativeWriteFailed)
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        if self.core.lock()?.wake(event_mask) == 0 {
            Ok(())
        } else {
            Err(FmqQueueError::NativeWakeFailed)
        }
    }

    pub fn descriptor_handle(&self) -> FmqDescriptorHandle {
        FmqDescriptorHandle {
            core: Arc::clone(&self.core),
        }
    }
}

impl FmqReader {
    pub fn create(num_bytes: usize, configure_event_flag: bool) -> Result<Self, FmqQueueError> {
        Ok(Self {
            core: FmqQueueCore::create(num_bytes, configure_event_flag)?,
        })
    }

    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, FmqQueueError> {
        self.core
            .lock()?
            .read_checked(data)
            .map_err(|status| match status {
                -1 => FmqQueueError::NativeReadInvalidArgument,
                _ => FmqQueueError::NativeReadFailed,
            })
    }

    pub fn available_to_read(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.available_to_read())
    }

    pub fn current_fill(&self) -> Result<usize, FmqQueueError> {
        self.available_to_read()
    }

    pub fn clear(&self) -> Result<usize, FmqQueueError> {
        self.core
            .lock()?
            .clear()
            .map_err(|_| FmqQueueError::NativeReadFailed)
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        if self.core.lock()?.wake(event_mask) == 0 {
            Ok(())
        } else {
            Err(FmqQueueError::NativeWakeFailed)
        }
    }

    pub fn descriptor_handle(&self) -> FmqDescriptorHandle {
        FmqDescriptorHandle {
            core: Arc::clone(&self.core),
        }
    }
}

impl FmqPeerWriter {
    pub fn write_checked(&self, bytes: &[u8]) -> Result<usize, FmqQueueError> {
        self.core
            .lock()?
            .write_checked(bytes)
            .map_err(|status| match status {
                -1 => FmqQueueError::NativeWriteInvalidArgument,
                _ => FmqQueueError::NativeWriteFailed,
            })
    }

    pub fn available_to_write(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.available_to_write())
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        if self.core.lock()?.wake(event_mask) == 0 {
            Ok(())
        } else {
            Err(FmqQueueError::NativeWakeFailed)
        }
    }
}

impl FmqPeerReader {
    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, FmqQueueError> {
        self.core
            .lock()?
            .read_checked(data)
            .map_err(|status| match status {
                -1 => FmqQueueError::NativeReadInvalidArgument,
                _ => FmqQueueError::NativeReadFailed,
            })
    }

    pub fn available_to_read(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.available_to_read())
    }
}

impl FmqWaiter {
    pub fn wait(&self, event_mask: u32, timeout_ns: i64) -> Result<FmqWaitOutcome, FmqQueueError> {
        match self.core.lock()?.wait(event_mask, timeout_ns) {
            Ok(state) => Ok(FmqWaitOutcome::Signaled(state)),
            Err(-110) => Ok(FmqWaitOutcome::TimedOut),
            Err(_) => Err(FmqQueueError::NativeWaitFailed),
        }
    }

    pub fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        if self.core.lock()?.wake(event_mask) == 0 {
            Ok(())
        } else {
            Err(FmqQueueError::NativeWakeFailed)
        }
    }
}

impl FmqDescriptorHandle {
    #[cfg(test)]
    pub(crate) fn open_peer_writer(&self) -> Result<FmqPeerWriter, FmqQueueError> {
        Ok(FmqPeerWriter {
            core: self.core.clone_endpoint()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn open_peer_reader(&self) -> Result<FmqPeerReader, FmqQueueError> {
        Ok(FmqPeerReader {
            core: self.core.clone_endpoint()?,
        })
    }

    pub fn open_waiter(&self) -> Result<FmqWaiter, FmqQueueError> {
        Ok(FmqWaiter {
            core: self.core.clone_endpoint()?,
        })
    }
    pub fn quantum_result(&self) -> Result<i32, FmqQueueError> {
        Ok(self.core.lock()?.quantum())
    }
    pub fn flags_result(&self) -> Result<i32, FmqQueueError> {
        Ok(self.core.lock()?.flags())
    }
    pub fn grantor_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.grantor_count())
    }
    pub fn grantor_at_result(&self, index: usize) -> Result<(i32, i32, i64), FmqQueueError> {
        self.core
            .lock()?
            .grantor_at(index)
            .ok_or(FmqQueueError::DescriptorGrantorUnavailable)
    }
    pub fn fd_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.fd_count())
    }
    pub fn dup_fd_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        let fd = self.core.lock()?.dup_fd_at(index);
        if fd < 0 {
            Err(FmqQueueError::DescriptorFdDupFailed)
        } else {
            Ok(fd)
        }
    }
    pub fn int_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.core.lock()?.int_count())
    }
    pub fn int_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        self.core
            .lock()?
            .int_at(index)
            .ok_or(FmqQueueError::DescriptorIntUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_types_and_descriptor_peers_preserve_direction() {
        let writer = FmqWriter::create(64, false).unwrap();
        let peer_reader = writer.descriptor_handle().open_peer_reader().unwrap();
        assert_eq!(writer.write_checked(&[1, 2, 3]).unwrap(), 3);
        let mut out = [0u8; 4];
        assert_eq!(peer_reader.read_into(&mut out).unwrap(), 3);
        assert_eq!(&out[..3], &[1, 2, 3]);

        let reader = FmqReader::create(64, false).unwrap();
        let peer_writer = reader.descriptor_handle().open_peer_writer().unwrap();
        assert_eq!(peer_writer.write_checked(&[4, 5]).unwrap(), 2);
        let mut out = [0u8; 4];
        assert_eq!(reader.read_into(&mut out).unwrap(), 2);
        assert_eq!(&out[..2], &[4, 5]);
    }
}
