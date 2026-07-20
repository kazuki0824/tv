//! frontend adapterが共有するPOSIX/Linux userspace ABI断片。
//!
//! ここはABI断片だけを持つ。frontend lifecycle、worker制御、stream状態は所有しない。

use std::io;

extern "C" {
    pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    pub fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

pub const POLLIN: i16 = 0x0001;
pub const POLLERR: i16 = 0x0008;
pub const POLLHUP: i16 = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

pub fn poll_error_is_interrupted(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Interrupted
}

pub fn last_errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}
