//! frontend adapterが共有するPOSIX/Linux userspace ABI断片。
//!
//! syscall ABIはLineageOS/AOSPが提供するnix/libc定義を使用し、手書きのextern宣言を持たない。

use std::io;

pub use nix::libc::{ioctl, poll, read};
pub type PollFd = nix::libc::pollfd;

pub const POLLIN: i16 = nix::libc::POLLIN;
pub const POLLERR: i16 = nix::libc::POLLERR;
pub const POLLHUP: i16 = nix::libc::POLLHUP;
pub const POLLNVAL: i16 = nix::libc::POLLNVAL;

pub fn poll_error_is_interrupted(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Interrupted
}

pub fn last_errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}
