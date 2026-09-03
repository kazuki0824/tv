use std::collections::VecDeque;
use std::fs::File;
use std::os::fd::IntoRawFd;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FmqQueueError {
    NativeCreateFailed,
    NativeWriteInvalidArgument,
    NativeWriteFailed,
    NativeReadZero,
    NativeClearBufferAllocationFailed,
    NativeClearReadFailed,
    NativeWakeFailed,
    DescriptorGrantorUnavailable,
    DescriptorFdDupFailed,
    DescriptorIntUnavailable,
}

pub struct FmqQueue {
    capacity: usize,
    bytes: Mutex<VecDeque<u8>>,
}

impl FmqQueue {
    pub fn create(num_bytes: usize, _configure_event_flag: bool) -> Result<Self, FmqQueueError> {
        if num_bytes == 0 {
            return Err(FmqQueueError::NativeCreateFailed);
        }
        Ok(Self {
            capacity: num_bytes,
            bytes: Mutex::new(VecDeque::with_capacity(num_bytes)),
        })
    }

    pub fn read_into(&self, data: &mut [u8]) -> Result<usize, FmqQueueError> {
        let mut bytes = self
            .bytes
            .lock()
            .map_err(|_| FmqQueueError::NativeReadZero)?;
        let count = data.len().min(bytes.len());
        for slot in data.iter_mut().take(count) {
            *slot = bytes.pop_front().ok_or(FmqQueueError::NativeReadZero)?;
        }
        Ok(count)
    }

    pub fn write_checked(&self, data: &[u8]) -> Result<usize, FmqQueueError> {
        let mut bytes = self
            .bytes
            .lock()
            .map_err(|_| FmqQueueError::NativeWriteFailed)?;
        if data.len() > self.capacity.saturating_sub(bytes.len()) {
            return Err(FmqQueueError::NativeWriteFailed);
        }
        bytes.extend(data.iter().copied());
        Ok(data.len())
    }

    pub fn clear(&self) -> Result<(), FmqQueueError> {
        self.bytes
            .lock()
            .map_err(|_| FmqQueueError::NativeClearReadFailed)?
            .clear();
        Ok(())
    }

    pub fn current_fill(&self) -> Result<usize, FmqQueueError> {
        self.available_to_read_result()
    }

    pub fn wake(&self, _event_mask: u32) -> Result<(), FmqQueueError> {
        Ok(())
    }

    pub fn available_to_read_result(&self) -> Result<usize, FmqQueueError> {
        self.bytes
            .lock()
            .map(|bytes| bytes.len())
            .map_err(|_| FmqQueueError::NativeReadZero)
    }

    pub fn available_to_write_result(&self) -> Result<usize, FmqQueueError> {
        self.bytes
            .lock()
            .map(|bytes| self.capacity.saturating_sub(bytes.len()))
            .map_err(|_| FmqQueueError::NativeWriteFailed)
    }

    pub fn quantum_result(&self) -> Result<i32, FmqQueueError> {
        Ok(1)
    }

    pub fn flags_result(&self) -> Result<i32, FmqQueueError> {
        Ok(1)
    }

    pub fn grantor_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(1)
    }

    pub fn grantor_at_result(&self, index: usize) -> Result<(i32, i32, i64), FmqQueueError> {
        if index != 0 {
            return Err(FmqQueueError::DescriptorGrantorUnavailable);
        }
        let extent = i64::try_from(self.capacity)
            .map_err(|_| FmqQueueError::DescriptorGrantorUnavailable)?;
        Ok((0, 0, extent))
    }

    pub fn fd_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(1)
    }

    pub fn dup_fd_at_result(&self, index: usize) -> Result<i32, FmqQueueError> {
        if index != 0 {
            return Err(FmqQueueError::DescriptorFdDupFailed);
        }
        File::open("/dev/null")
            .map(IntoRawFd::into_raw_fd)
            .map_err(|_| FmqQueueError::DescriptorFdDupFailed)
    }

    pub fn int_count_result(&self) -> Result<usize, FmqQueueError> {
        Ok(0)
    }

    pub fn int_at_result(&self, _index: usize) -> Result<i32, FmqQueueError> {
        Err(FmqQueueError::DescriptorIntUnavailable)
    }
}
