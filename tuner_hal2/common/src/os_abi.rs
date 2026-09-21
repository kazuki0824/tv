//! frontend adapterが共有するPOSIX/Linux userspace ABI断片。
//!
//! syscall ABIはLineageOS/AOSPが提供するnix/libc定義を使用し、手書きのextern宣言を持たない。

use std::io;

pub use nix::libc::{poll, read};
pub type PollFd = nix::libc::pollfd;

#[cfg(target_os = "android")]
type IoctlRequest = nix::libc::c_int;
#[cfg(not(target_os = "android"))]
type IoctlRequest = nix::libc::c_ulong;

#[cfg(target_os = "android")]
fn ioctl_request(request: u64) -> IoctlRequest {
    request as IoctlRequest
}

#[cfg(all(not(target_os = "android"), target_pointer_width = "64"))]
fn ioctl_request(request: u64) -> IoctlRequest {
    request
}

#[cfg(all(not(target_os = "android"), target_pointer_width = "32"))]
fn ioctl_request(request: u64) -> IoctlRequest {
    request as IoctlRequest
}

/// # Safety
///
/// `fd` と `arg` は `request` が要求するLinux ioctl ABIを満たさなければならない。
pub unsafe fn raw_ioctl_ptr<T>(fd: i32, request: u64, arg: *mut T) -> i32 {
    // SAFETY: 呼出元がfd、request、payloadの有効性を保証する。
    unsafe { nix::libc::ioctl(fd, ioctl_request(request), arg) }
}

/// # Safety
///
/// `fd` と `request` は引数なしLinux ioctl ABIを満たさなければならない。
pub unsafe fn raw_ioctl_noarg(fd: i32, request: u64) -> i32 {
    // SAFETY: 呼出元がfdとrequestの有効性を保証する。
    unsafe { nix::libc::ioctl(fd, ioctl_request(request)) }
}

/// # Safety
///
/// `fd`、`request`、`arg` はscalar引数を取るLinux ioctl ABIを満たさなければならない。
pub unsafe fn raw_ioctl_word(fd: i32, request: u64, arg: u32) -> i32 {
    // SAFETY: 呼出元がfd、request、scalar payloadの有効性を保証する。
    unsafe { nix::libc::ioctl(fd, ioctl_request(request), arg) }
}

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
