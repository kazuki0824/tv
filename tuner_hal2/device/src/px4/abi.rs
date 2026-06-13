//! px4_drv legacy ioctl ABI定数とC layout構造体。
//!
//! 再利用するdriver ABI断片として置く。旧backend lifecycleやworker control層は意図的に持ち込まない。

use core::mem::size_of;

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

pub const fn ioc(dir: u32, typ: u32, nr: u32, size: u32) -> u64 {
    ((dir << IOC_DIRSHIFT) | (typ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT)) as u64
}
pub const fn io(typ: u32, nr: u32) -> u64 { ioc(IOC_NONE, typ, nr, 0) }
pub const fn iow<T>(typ: u32, nr: u32) -> u64 { ioc(IOC_WRITE, typ, nr, size_of::<T>() as u32) }
pub const fn ior<T>(typ: u32, nr: u32) -> u64 { ioc(IOC_READ, typ, nr, size_of::<T>() as u32) }

pub const PTX_IOCTL_TYPE_BASIC: u32 = 0x8d;
pub const PTX_IOCTL_TYPE_EXT: u32 = 0xe7;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PtxFreq {
    pub freq_no: i32,
    pub slot: i32,
}

pub const PTX_SET_CHANNEL: u64 = iow::<PtxFreq>(PTX_IOCTL_TYPE_BASIC, 0x01);
pub const PTX_START_STREAMING: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x02);
pub const PTX_STOP_STREAMING: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x03);
pub const PTX_GET_CNR: u64 = ior::<u32>(PTX_IOCTL_TYPE_BASIC, 0x04);
pub const PTX_ENABLE_LNB_POWER: u64 = iow::<i32>(PTX_IOCTL_TYPE_BASIC, 0x05);
pub const PTX_DISABLE_LNB_POWER: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x06);
pub const PTX_SET_SYSTEM_MODE: u64 = iow::<u32>(PTX_IOCTL_TYPE_BASIC, 0x0b);
pub const PTXT_SET_LNB_VOLTAGE: u64 = iow::<i32>(PTX_IOCTL_TYPE_EXT, 0x05);

pub const O_NONBLOCK: i32 = 0x800;
pub const ERRNO_EINVAL: i32 = 22;
pub const ERRNO_ENOTTY: i32 = 25;
pub const ERRNO_ENOSYS: i32 = 38;

pub const PTX_ISDB_T_SYSTEM: u32 = 0x0000_0010;
pub const PTX_ISDB_S_SYSTEM: u32 = 0x0000_0020;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px4_ioctl_numbers_are_stable() {
        assert_eq!(PTX_SET_CHANNEL, iow::<PtxFreq>(PTX_IOCTL_TYPE_BASIC, 0x01));
        assert_eq!(PTX_START_STREAMING, io(PTX_IOCTL_TYPE_BASIC, 0x02));
        assert_eq!(PTX_SET_SYSTEM_MODE, iow::<u32>(PTX_IOCTL_TYPE_BASIC, 0x0b));
    }
}
