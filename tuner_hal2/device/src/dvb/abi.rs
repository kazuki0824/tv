//! Linux DVB / earth_pt1 ioctl ABI定数とC layout構造体。
//!
//! 再利用するABI断片だけを置く。旧backend lifecycle/control層はtuner_hal2へコピーしない。

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
    ((dir << IOC_DIRSHIFT) | (typ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT))
        as u64
}
pub const fn io(typ: u32, nr: u32) -> u64 {
    ioc(IOC_NONE, typ, nr, 0)
}
pub const fn ior<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_READ, typ, nr, size_of::<T>() as u32)
}
pub const fn iow<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_WRITE, typ, nr, size_of::<T>() as u32)
}

pub const FE_IOCTL_TYPE: u32 = b'o' as u32;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DtvPropertyBuffer {
    pub data: [u8; 32],
    pub len: u32,
    pub reserved1: [u32; 3],
    pub reserved2: *mut core::ffi::c_void,
}

impl DtvPropertyBuffer {
    pub fn read_len_unaligned(&self) -> u32 {
        // 安全性: DtvPropertyBufferは#[repr(C, packed)]である。packed fieldの読取にはread_unalignedを使い、pointerは有効参照から導出する。
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.len)) }
    }

    pub fn read_data_unaligned(&self) -> [u8; 32] {
        // 安全性: read_len_unaligned() と同じpacked field規則に従う。
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.data)) }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union DtvPropertyUnion {
    pub data: u32,
    pub buffer: DtvPropertyBuffer,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DtvProperty {
    pub cmd: u32,
    pub reserved: [u32; 3],
    pub u: DtvPropertyUnion,
    pub result: i32,
}

impl DtvProperty {
    pub fn with_data(cmd: u32, value: u32) -> Self {
        Self {
            cmd,
            reserved: [0; 3],
            u: DtvPropertyUnion { data: value },
            result: 0,
        }
    }

    pub fn read_data_unaligned(&self) -> u32 {
        // 安全性: DtvPropertyはpackedである。union storageをu32として読む処理はLinux DVB dtv_property ABI data fieldに合わせる。
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.u) as *const u32) }
    }

    pub fn read_buffer_unaligned(&self) -> DtvPropertyBuffer {
        // 安全性: DtvPropertyはpackedであり、buffer variant bytesをcopyして取り出す。
        unsafe {
            core::ptr::read_unaligned(core::ptr::addr_of!(self.u) as *const DtvPropertyBuffer)
        }
    }

    pub fn read_result_unaligned(&self) -> i32 {
        // 安全性: DtvPropertyはpackedであり、fieldをunaligned copyで取り出す。
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.result)) }
    }
}

#[repr(C)]
pub struct DtvProperties {
    pub num: u32,
    pub props: *mut DtvProperty,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DmxPesFilterParams {
    pub pid: u16,
    pub input: u32,
    pub output: u32,
    pub pes_type: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DvbFrontendInfo {
    pub name: [u8; 128],
    pub fe_type: u32,
    pub frequency_min: u32,
    pub frequency_max: u32,
    pub frequency_stepsize: u32,
    pub frequency_tolerance: u32,
    pub symbol_rate_min: u32,
    pub symbol_rate_max: u32,
    pub symbol_rate_tolerance: u32,
    pub notifier_delay: u32,
    pub caps: u32,
}

pub const FE_SET_PROPERTY: u64 = iow::<DtvProperties>(FE_IOCTL_TYPE, 82);
pub const FE_GET_PROPERTY: u64 = ior::<DtvProperties>(FE_IOCTL_TYPE, 83);
pub const FE_READ_STATUS: u64 = ior::<u32>(FE_IOCTL_TYPE, 69);
pub const FE_READ_SIGNAL_STRENGTH: u64 = ior::<u16>(FE_IOCTL_TYPE, 71);
pub const FE_READ_SNR: u64 = ior::<u16>(FE_IOCTL_TYPE, 72);
pub const FE_SET_VOLTAGE: u64 = io(FE_IOCTL_TYPE, 67);
pub const FE_GET_INFO: u64 = ior::<DvbFrontendInfo>(FE_IOCTL_TYPE, 61);

pub const DMX_SET_PES_FILTER: u64 = iow::<DmxPesFilterParams>(FE_IOCTL_TYPE, 44);
pub const DMX_SET_SOURCE: u64 = iow::<u32>(FE_IOCTL_TYPE, 49);
pub const DMX_STOP: u64 = io(FE_IOCTL_TYPE, 42);

pub const DTV_TUNE: u32 = 1;
pub const DTV_CLEAR: u32 = 2;
pub const DTV_FREQUENCY: u32 = 3;
pub const DTV_BANDWIDTH_HZ: u32 = 5;
pub const DTV_SYMBOL_RATE: u32 = 8;
pub const DTV_DELIVERY_SYSTEM: u32 = 17;
pub const DTV_STREAM_ID: u32 = 42;
pub const DTV_ENUM_DELSYS: u32 = 44;
pub const NO_STREAM_ID_FILTER: u32 = u32::MAX;

pub const FE_HAS_SIGNAL: u32 = 0x01;
pub const FE_HAS_CARRIER: u32 = 0x02;
pub const FE_HAS_VITERBI: u32 = 0x04;
pub const FE_HAS_SYNC: u32 = 0x08;
pub const FE_HAS_LOCK: u32 = 0x10;

pub const SYS_DVBS2: u32 = 6;
pub const SYS_ISDBT: u32 = 8;
pub const SYS_ISDBS: u32 = 9;

pub const SEC_VOLTAGE_13: u32 = 0;
pub const SEC_VOLTAGE_18: u32 = 1;
pub const SEC_VOLTAGE_OFF: u32 = 2;
pub const O_NONBLOCK: i32 = 0x800;
pub const DMX_IN_FRONTEND: u32 = 0;
pub const DMX_OUT_TS_TAP: u32 = 2;
pub const DMX_PES_OTHER: u32 = 20;
pub const DMX_IMMEDIATE_START: u32 = 1;

pub fn dvb_frontend_name(info: &DvbFrontendInfo) -> String {
    let nul = info
        .name
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(info.name.len());
    String::from_utf8_lossy(&info.name[..nul])
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvb_ioctl_numbers_are_stable() {
        assert_eq!(DTV_FREQUENCY, 3);
        assert_eq!(DTV_BANDWIDTH_HZ, 5);
        assert_eq!(DTV_DELIVERY_SYSTEM, 17);
        assert_eq!(DTV_STREAM_ID, 42);
        assert_eq!(FE_SET_PROPERTY, iow::<DtvProperties>(FE_IOCTL_TYPE, 82));
    }
}
