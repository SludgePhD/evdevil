//! `linux/uinput.h`.

#![expect(dead_code)] // some items not yet exposed

use std::ffi::{c_char, c_int, c_uint};

use linux_ioctl::{_IO, _IOC, _IOC_READ, _IOR, _IOW, _IOWR, Ioctl};

use super::input::{ff_effect, input_absinfo, input_id};

pub const UINPUT_VERSION: u8 = 5;
pub const UINPUT_MAX_NAME_SIZE: usize = 80;

#[repr(C)]
pub struct uinput_ff_upload {
    pub request_id: u32,
    pub retval: i32,
    pub effect: ff_effect,
    pub old: ff_effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct uinput_ff_erase {
    pub request_id: u32,
    pub retval: i32,
    pub effect_id: u32,
}

pub const UINPUT_IOCTL_BASE: u8 = b'U';
pub const UI_DEV_CREATE: Ioctl = _IO(UINPUT_IOCTL_BASE, 1);
pub const UI_DEV_DESTROY: Ioctl = _IO(UINPUT_IOCTL_BASE, 2);

#[repr(C)]
pub struct uinput_setup {
    pub id: input_id,
    pub name: [c_char; UINPUT_MAX_NAME_SIZE],
    pub ff_effects_max: u32,
}

pub const UI_DEV_SETUP: Ioctl<*const uinput_setup> = _IOW(UINPUT_IOCTL_BASE, 3);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct uinput_abs_setup {
    pub code: u16,
    /* filler: u16 */
    pub absinfo: input_absinfo,
}

pub const UI_ABS_SETUP: Ioctl<*const uinput_abs_setup> = _IOW(UINPUT_IOCTL_BASE, 4);

#[cfg(target_os = "freebsd")]
mod ioctls {
    use linux_ioctl::IOC_VOID;

    use super::*;

    const fn _IOWINT(group: u8, nr: u8) -> Ioctl<c_int> {
        _IOC(IOC_VOID, group, nr, size_of::<c_int>())
    }

    pub const UI_SET_EVBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 100);
    pub const UI_SET_KEYBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 101);
    pub const UI_SET_RELBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 102);
    pub const UI_SET_ABSBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 103);
    pub const UI_SET_MSCBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 104);
    pub const UI_SET_LEDBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 105);
    pub const UI_SET_SNDBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 106);
    pub const UI_SET_FFBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 107);
    pub const UI_SET_PHYS: Ioctl<*const c_char> = _IO(UINPUT_IOCTL_BASE, 108).with_arg();
    pub const UI_SET_SWBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 109);
    pub const UI_SET_PROPBIT: Ioctl<c_int> = _IOWINT(UINPUT_IOCTL_BASE, 110);
}

#[cfg(not(target_os = "freebsd"))]
mod ioctls {
    use super::*;
    pub const UI_SET_EVBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 100).with_direct_arg();
    pub const UI_SET_KEYBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 101).with_direct_arg();
    pub const UI_SET_RELBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 102).with_direct_arg();
    pub const UI_SET_ABSBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 103).with_direct_arg();
    pub const UI_SET_MSCBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 104).with_direct_arg();
    pub const UI_SET_LEDBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 105).with_direct_arg();
    pub const UI_SET_SNDBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 106).with_direct_arg();
    pub const UI_SET_FFBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 107).with_direct_arg();
    pub const UI_SET_PHYS: Ioctl<*const c_char> = _IOW(UINPUT_IOCTL_BASE, 108).with_direct_arg();
    pub const UI_SET_SWBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 109).with_direct_arg();
    pub const UI_SET_PROPBIT: Ioctl<c_int> = _IOW(UINPUT_IOCTL_BASE, 110).with_direct_arg();
}

pub use ioctls::*;

pub const UI_BEGIN_FF_UPLOAD: Ioctl<*mut uinput_ff_upload> = _IOWR(UINPUT_IOCTL_BASE, 200);
pub const UI_END_FF_UPLOAD: Ioctl<*const uinput_ff_upload> = _IOW(UINPUT_IOCTL_BASE, 201);
pub const UI_BEGIN_FF_ERASE: Ioctl<*mut uinput_ff_erase> = _IOWR(UINPUT_IOCTL_BASE, 202);
pub const UI_END_FF_ERASE: Ioctl<*const uinput_ff_erase> = _IOW(UINPUT_IOCTL_BASE, 203);

pub const fn UI_GET_SYSNAME(len: usize) -> Ioctl<*mut c_char> {
    _IOC(_IOC_READ, UINPUT_IOCTL_BASE, 44, len)
}

pub const UI_GET_VERSION: Ioctl<*mut c_uint> = _IOR(UINPUT_IOCTL_BASE, 45);
