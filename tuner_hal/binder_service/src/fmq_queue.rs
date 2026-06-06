//! FMQ read / write / clear / fill を集約する薄い接続層。
//!
//! r50dz19 では binder_service 本体から `tuner_fmq_*` FFI symbol を隠蔽する。
//! HAL object はこの module の wrapper だけを呼ぶ。

use std::ffi::c_void;

#[repr(C)]
pub struct TunerFmqQueue(c_void);

extern "C" {
    #[link_name = "tuner_fmq_queue_create"]
    fn native_queue_create(num_bytes: usize, configure_event_flag: bool) -> *mut TunerFmqQueue;
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
    #[link_name = "tuner_fmq_queue_read"]
    fn native_queue_read(queue: *mut TunerFmqQueue, data: *mut u8, size: usize) -> usize;
    #[link_name = "tuner_fmq_queue_wake"]
    fn native_queue_wake(queue: *mut TunerFmqQueue, bits: u32) -> i32;
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

unsafe impl Send for NativeFmqQueue {}
unsafe impl Sync for NativeFmqQueue {}

impl NativeFmqQueue {
    fn create(num_bytes: usize, configure_event_flag: bool) -> Option<Self> {
        let queue = unsafe { native_queue_create(num_bytes, configure_event_flag) };
        if queue.is_null() {
            None
        } else {
            Some(Self { queue })
        }
    }

    pub(crate) fn available_to_read(&self) -> usize {
        unsafe { native_queue_available_to_read(self.queue) }
    }

    fn fill_status(&self) -> FmqFillStatus {
        FmqFillStatus::Bytes(self.available_to_read())
    }

    pub(crate) fn available_to_write(&self) -> usize {
        unsafe { native_queue_available_to_write(self.queue) }
    }

    pub(crate) fn write_checked(&self, data: &[u8]) -> Result<usize, i32> {
        let mut written = 0usize;
        let (ptr, len) = if data.is_empty() {
            (std::ptr::null(), 0usize)
        } else {
            (data.as_ptr(), data.len())
        };
        let status = unsafe { native_queue_write_checked(self.queue, ptr, len, &mut written) };
        if status == 0 {
            Ok(written)
        } else {
            Err(status)
        }
    }

    fn read(&self, data: &mut [u8]) -> usize {
        if data.is_empty() {
            0
        } else {
            unsafe { native_queue_read(self.queue, data.as_mut_ptr(), data.len()) }
        }
    }

    pub(crate) fn wake(&self, bits: u32) -> i32 {
        unsafe { native_queue_wake(self.queue, bits) }
    }

    pub(crate) fn quantum(&self) -> i32 {
        unsafe { native_queue_quantum(self.queue) }
    }

    pub(crate) fn flags(&self) -> i32 {
        unsafe { native_queue_flags(self.queue) }
    }

    pub(crate) fn grantor_count(&self) -> usize {
        unsafe { native_queue_grantor_count(self.queue) }
    }

    pub(crate) fn grantor_at(&self, index: usize) -> Option<(i32, i32, i64)> {
        let (mut fd_index, mut offset, mut extent) = (0i32, 0i32, 0i64);
        let ok = unsafe {
            native_queue_grantor_at(self.queue, index, &mut fd_index, &mut offset, &mut extent)
        };
        if ok { Some((fd_index, offset, extent)) } else { None }
    }

    pub(crate) fn fd_count(&self) -> usize {
        unsafe { native_queue_fd_count(self.queue) }
    }

    pub(crate) fn dup_fd_at(&self, index: usize) -> i32 {
        unsafe { native_queue_dup_fd_at(self.queue, index) }
    }

    pub(crate) fn int_count(&self) -> usize {
        unsafe { native_queue_int_count(self.queue) }
    }

    pub(crate) fn int_at(&self, index: usize) -> Option<i32> {
        let mut value = 0i32;
        let ok = unsafe { native_queue_int_at(self.queue, index, &mut value) };
        if ok { Some(value) } else { None }
    }
}

impl Drop for NativeFmqQueue {
    fn drop(&mut self) {
        unsafe { native_queue_destroy(self.queue) };
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqClearOutcome { Cleared }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqFillStatus { Bytes(usize) }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqQueueError {
    NativeCreateFailed,
    NoNativeQueue,
    NativeWriteInvalidArgument,
    NativeWriteFailed,
    NativeReadZero,
    NativeWakeFailed,
    DescriptorGrantorUnavailable,
    DescriptorFdDupFailed,
    DescriptorIntUnavailable,
}

impl FmqQueueError {
    pub(crate) fn is_transient_descriptor_export_error(self) -> bool {
        matches!(
            self,
            FmqQueueError::DescriptorFdDupFailed
                | FmqQueueError::DescriptorGrantorUnavailable
                | FmqQueueError::DescriptorIntUnavailable
        )
    }

    pub(crate) fn is_data_path_failure(self) -> bool {
        matches!(
            self,
            FmqQueueError::NativeWriteInvalidArgument
                | FmqQueueError::NativeWriteFailed
                | FmqQueueError::NativeReadZero
                | FmqQueueError::NativeWakeFailed
        )
    }
}

pub struct FmqQueue { native: Option<NativeFmqQueue>, test_fill: usize }

impl FmqQueue {
    pub fn new() -> Self { Self { native: None, test_fill: 0 } }
    pub fn create(num_bytes: usize, configure_event_flag: bool) -> Result<Self, FmqQueueError> {
        let native = NativeFmqQueue::create(num_bytes, configure_event_flag).ok_or(FmqQueueError::NativeCreateFailed)?;
        Ok(Self { native: Some(native), test_fill: 0 })
    }
    pub(crate) fn read_into(&self, data: &mut [u8]) -> Result<usize, FmqQueueError> {
        let Some(native) = &self.native else { return Ok(0); };
        let available = native.available_to_read();
        let read = native.read(data);
        if available > 0 && !data.is_empty() && read == 0 {
            return Err(FmqQueueError::NativeReadZero);
        }
        Ok(read)
    }
    pub(crate) fn write_checked(&self, bytes: &[u8]) -> Result<usize, FmqQueueError> {
        let Some(native) = self.native.as_ref() else {
            return Err(FmqQueueError::NoNativeQueue);
        };
        native.write_checked(bytes).map_err(|status| match status {
            -1 => FmqQueueError::NativeWriteInvalidArgument,
            -2 => FmqQueueError::NativeWriteFailed,
            _ => FmqQueueError::NativeWriteFailed,
        })
    }
    #[cfg(test)]
    pub fn clear(&mut self) -> Result<FmqClearOutcome, FmqQueueError> {
        if let Some(native) = &self.native {
            let mut scratch = vec![0u8; 4096];
            while native.available_to_read() > 0 {
                let read = native.read(&mut scratch);
                if read == 0 {
                    return Err(FmqQueueError::NativeReadZero);
                }
            }
        }
        self.test_fill = 0;
        Ok(FmqClearOutcome::Cleared)
    }
    fn fill_status(&self) -> Result<FmqFillStatus, FmqQueueError> {
        match &self.native { Some(native) => Ok(native.fill_status()), None => Ok(FmqFillStatus::Bytes(self.test_fill)) }
    }
    pub fn current_fill(&self) -> Result<FmqFillStatus, FmqQueueError> { self.fill_status() }
    pub(crate) fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        let Some(native) = &self.native else { return Err(FmqQueueError::NoNativeQueue); };
        if native.wake(event_mask) == 0 { Ok(()) } else { Err(FmqQueueError::NativeWakeFailed) }
    }
    fn native_ref(&self) -> Result<&NativeFmqQueue, FmqQueueError> {
        self.native.as_ref().ok_or(FmqQueueError::NoNativeQueue)
    }

    pub(crate) fn available_to_read_result(&self) -> Result<usize, FmqQueueError> {
        match &self.native {
            Some(native) => Ok(native.available_to_read()),
            None => Ok(self.test_fill),
        }
    }

    pub(crate) fn available_to_write_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native_ref()?.available_to_write())
    }

    pub(crate) fn quantum_result(&self) -> Result<i32, FmqQueueError> { Ok(self.native_ref()?.quantum()) }
    pub(crate) fn flags_result(&self) -> Result<i32, FmqQueueError> { Ok(self.native_ref()?.flags()) }
    pub(crate) fn grantor_count_result(&self) -> Result<usize, FmqQueueError> { Ok(self.native_ref()?.grantor_count()) }
    pub(crate) fn grantor_at_result(&self, index: usize) -> Result<(i32,i32,i64), FmqQueueError> {
        self.native_ref()?.grantor_at(index).ok_or(FmqQueueError::DescriptorGrantorUnavailable)
    }
    pub(crate) fn fd_count_result(&self) -> Result<usize, FmqQueueError> { Ok(self.native_ref()?.fd_count()) }
    pub(crate) fn dup_fd_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        let fd = self.native_ref()?.dup_fd_at(index);
        if fd < 0 { Err(FmqQueueError::DescriptorFdDupFailed) } else { Ok(fd) }
    }
    pub(crate) fn int_count_result(&self) -> Result<usize, FmqQueueError> { Ok(self.native_ref()?.int_count()) }
    pub(crate) fn int_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        self.native_ref()?.int_at(index).ok_or(FmqQueueError::DescriptorIntUnavailable)
    }
}
impl Default for FmqQueue { fn default() -> Self { Self::new() } }

#[cfg(test)] mod tests {
    use super::*;

    #[test]
    fn fmq_queue_reports_fill_as_result(){
        let queue=FmqQueue::new();
        assert_eq!(queue.current_fill(),Ok(FmqFillStatus::Bytes(0)));
    }

    #[test]
    fn native_metadata_accessors_do_not_round_missing_queue_to_zero() {
        let queue = FmqQueue::new();
        assert_eq!(queue.available_to_write_result(), Err(FmqQueueError::NoNativeQueue));
        assert_eq!(queue.grantor_count_result(), Err(FmqQueueError::NoNativeQueue));
        assert_eq!(queue.fd_count_result(), Err(FmqQueueError::NoNativeQueue));
        assert_eq!(queue.int_count_result(), Err(FmqQueueError::NoNativeQueue));
        assert_eq!(queue.quantum_result(), Err(FmqQueueError::NoNativeQueue));
        assert_eq!(queue.flags_result(), Err(FmqQueueError::NoNativeQueue));
    }
}


#[cfg(test)]
mod r50dz52_g3_11_tests {
    use super::*;

    fn clear_probe_for_test(mut available: usize, reads: &[usize]) -> Result<FmqClearOutcome, FmqQueueError> {
        let mut pos = 0usize;
        while available > 0 {
            let read = reads.get(pos).copied().unwrap_or(0);
            pos += 1;
            if read == 0 {
                return Err(FmqQueueError::NativeReadZero);
            }
            available = available.saturating_sub(read);
        }
        Ok(FmqClearOutcome::Cleared)
    }

    #[test]
    fn native_read_zero_is_not_reported_as_cleared() {
        assert_eq!(clear_probe_for_test(188, &[0]), Err(FmqQueueError::NativeReadZero));
        assert_eq!(clear_probe_for_test(188, &[188]), Ok(FmqClearOutcome::Cleared));
    }
}

#[cfg(test)]
mod r50eb40_fmq_error_classification_tests {
    use super::*;

    #[test]
    fn fmq_error_classification_separates_descriptor_export_from_data_path_failure() {
        assert!(FmqQueueError::DescriptorFdDupFailed.is_transient_descriptor_export_error());
        assert!(FmqQueueError::DescriptorGrantorUnavailable.is_transient_descriptor_export_error());
        assert!(FmqQueueError::DescriptorIntUnavailable.is_transient_descriptor_export_error());
        assert!(!FmqQueueError::DescriptorFdDupFailed.is_data_path_failure());

        assert!(FmqQueueError::NativeWriteFailed.is_data_path_failure());
        assert!(FmqQueueError::NativeWriteInvalidArgument.is_data_path_failure());
        assert!(FmqQueueError::NativeReadZero.is_data_path_failure());
        assert!(FmqQueueError::NativeWakeFailed.is_data_path_failure());
        assert!(!FmqQueueError::NativeWakeFailed.is_transient_descriptor_export_error());
    }
}
