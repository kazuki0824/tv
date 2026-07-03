//! HAL2 FMQ native shim wrapper。
//!
//! binder_service制御層は持たない。ここではlibfmq native shimへの最小Rust接続だけを保持し、配送commitやlifecycleはcontrol層で実装する。

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

// 安全性: NativeFmqQueue は tuner_fmq_queue_create が生成したopaque pointerを所有する。
// underlying libfmq queueはlibfmq/EventFlagで同期され、wrapperはnative object内部へのRust参照を公開しない。破棄はDropによる単一ownerで行う。
unsafe impl Send for NativeFmqQueue {}
// 安全性: 上記Send安全性注記に従う。共有accessはnative methodだけを呼ぶ
// opaque FMQ handleだけを操作し、native状態への可変Rust aliasingを公開しない。
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

    fn fill_bytes(&self) -> usize {
        self.available_to_read()
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
        if ok {
            Some((fd_index, offset, extent))
        } else {
            None
        }
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
        if ok {
            Some(value)
        } else {
            None
        }
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
    NativeWriteInvalidArgument,
    NativeWriteFailed,
    NativeReadZero,
    NativeWakeFailed,
    DescriptorGrantorUnavailable,
    DescriptorFdDupFailed,
    DescriptorIntUnavailable,
}

pub struct FmqQueue {
    native: NativeFmqQueue,
}

impl FmqQueue {
    pub fn create(num_bytes: usize, configure_event_flag: bool) -> Result<Self, FmqQueueError> {
        let native = NativeFmqQueue::create(num_bytes, configure_event_flag)
            .ok_or(FmqQueueError::NativeCreateFailed)?;
        Ok(Self { native })
    }
    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, FmqQueueError> {
        let available = self.native.available_to_read();
        let read = self.native.read(data);
        if available > 0 && !data.is_empty() && read == 0 {
            return Err(FmqQueueError::NativeReadZero);
        }
        Ok(read)
    }
    pub fn write_checked(&self, bytes: &[u8]) -> Result<usize, FmqQueueError> {
        self.native
            .write_checked(bytes)
            .map_err(|status| match status {
                -1 => FmqQueueError::NativeWriteInvalidArgument,
                -2 => FmqQueueError::NativeWriteFailed,
                _ => FmqQueueError::NativeWriteFailed,
            })
    }
    pub fn clear(&self) -> Result<(), FmqQueueError> {
        let mut scratch = vec![0u8; 4096];
        while self.native.available_to_read() > 0 {
            let read = self.native.read(&mut scratch);
            if read == 0 {
                return Err(FmqQueueError::NativeReadZero);
            }
        }
        Ok(())
    }
    pub fn current_fill(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.fill_bytes())
    }
    pub fn wake(&self, event_mask: u32) -> Result<(), FmqQueueError> {
        if self.native.wake(event_mask) == 0 {
            Ok(())
        } else {
            Err(FmqQueueError::NativeWakeFailed)
        }
    }

    pub fn available_to_read_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.available_to_read())
    }
    pub fn available_to_write_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.available_to_write())
    }
    pub fn quantum_result(&self) -> Result<i32, FmqQueueError> {
        Ok(self.native.quantum())
    }
    pub fn flags_result(&self) -> Result<i32, FmqQueueError> {
        Ok(self.native.flags())
    }
    pub fn grantor_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.grantor_count())
    }
    pub fn grantor_at_result(&self, index: usize) -> Result<(i32, i32, i64), FmqQueueError> {
        self.native
            .grantor_at(index)
            .ok_or(FmqQueueError::DescriptorGrantorUnavailable)
    }
    pub fn fd_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.fd_count())
    }
    pub fn dup_fd_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        let fd = self.native.dup_fd_at(index);
        if fd < 0 {
            Err(FmqQueueError::DescriptorFdDupFailed)
        } else {
            Ok(fd)
        }
    }
    pub fn int_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(self.native.int_count())
    }
    pub fn int_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        self.native
            .int_at(index)
            .ok_or(FmqQueueError::DescriptorIntUnavailable)
    }
}

#[cfg(test)]
mod fmq_clear_tests {
    use super::*;

    fn clear_probe_for_test(mut available: usize, reads: &[usize]) -> Result<(), FmqQueueError> {
        let mut pos = 0usize;
        while available > 0 {
            let read = reads.get(pos).copied().unwrap_or(0);
            pos += 1;
            if read == 0 {
                return Err(FmqQueueError::NativeReadZero);
            }
            available = available.saturating_sub(read);
        }
        Ok(())
    }

    #[test]
    fn native_read_zero_is_not_reported_as_cleared() {
        assert_eq!(
            clear_probe_for_test(188, &[0]),
            Err(FmqQueueError::NativeReadZero)
        );
        assert_eq!(clear_probe_for_test(188, &[188]), Ok(()));
    }
}

#[cfg(test)]
mod fmq_error_classification_tests {
    use super::*;

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
