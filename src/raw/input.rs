//! `linux/input.h`

use std::ffi::{c_char, c_int, c_uint, c_void};

use libc::timeval;
use linux_ioctl::{_IOC, _IOC_READ, _IOR, _IOW, Ioctl};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct input_event {
    pub time: timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

impl PartialEq for input_event {
    fn eq(&self, other: &Self) -> bool {
        self.time.tv_sec == other.time.tv_sec
            && self.time.tv_usec == other.time.tv_usec
            && self.type_ == other.type_
            && self.code == other.code
            && self.value == other.value
    }
}
impl Eq for input_event {}

#[expect(dead_code)] // not needed
pub const EV_VERSION: c_int = 0x010001;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct input_id {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct input_absinfo {
    pub value: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct input_keymap_entry {
    pub flags: u8,
    pub len: u8,
    pub index: u16,
    pub keycode: u32,
    pub scancode: [u8; 32],
}

pub const INPUT_KEYMAP_BY_INDEX: u8 = 1 << 0;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct input_mask {
    pub type_: u32,
    pub codes_size: u32,
    pub codes_ptr: u64,
}

/// Get driver version.
pub const EVIOCGVERSION: Ioctl<*mut c_int> = _IOR(b'E', 0x01);
/// Get device ID.
pub const EVIOCGID: Ioctl<*mut input_id> = _IOR(b'E', 0x02);
/// Get repeat settings.
pub const EVIOCGREP: Ioctl<*mut [c_uint; 2]> = _IOR(b'E', 0x03);
/// Set repeat settings.
pub const EVIOCSREP: Ioctl<*const [c_uint; 2]> = _IOW(b'E', 0x03);

/// Get keycode.
#[expect(dead_code)] // replaced by _V2 in 2010
pub const EVIOCGKEYCODE: Ioctl<*mut [c_uint; 2]> = _IOR(b'E', 0x04);
pub const EVIOCGKEYCODE_V2: Ioctl<*mut input_keymap_entry> = _IOR(b'E', 0x04);
/// Set keycode.
#[expect(dead_code)] // replaced by _V2 in 2010
pub const EVIOCSKEYCODE: Ioctl<*const [c_uint; 2]> = _IOW(b'E', 0x04);
pub const EVIOCSKEYCODE_V2: Ioctl<*const input_keymap_entry> = _IOW(b'E', 0x04);

/// Get device name.
pub const fn EVIOCGNAME(len: usize) -> Ioctl<*mut c_char> {
    _IOC(_IOC_READ, b'E', 0x06, len)
}
/// Get physical location.
pub const fn EVIOCGPHYS(len: usize) -> Ioctl<*mut c_char> {
    _IOC(_IOC_READ, b'E', 0x07, len)
}
pub const fn EVIOCGUNIQ(len: usize) -> Ioctl<*mut c_char> {
    _IOC(_IOC_READ, b'E', 0x08, len)
}
pub const fn EVIOCGPROP(len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x09, len)
}

pub const fn EVIOCGMTSLOTS(len: usize) -> Ioctl<*mut c_void> {
    use linux_ioctl::IOC_INOUT;

    if cfg!(target_os = "freebsd") {
        _IOC(IOC_INOUT, b'E', 0x0a, len)
    } else {
        // NB: declared as `_IOC_READ`, but writes the `code`
        _IOC(_IOC_READ, b'E', 0x0a, len)
    }
}

/// Get global key state.
pub const fn EVIOCGKEY(len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x18, len)
}
/// Get all LEDs.
pub const fn EVIOCGLED(len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x19, len)
}
/// Get all sounds state.
pub const fn EVIOCGSND(len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x1a, len)
}
/// Get all switch states.
pub const fn EVIOCGSW(len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x1b, len)
}

pub const fn EVIOCGBIT(ev: u8, len: usize) -> Ioctl<*mut c_void> {
    _IOC(_IOC_READ, b'E', 0x20 + ev, len)
}
pub const fn EVIOCGABS(abs: u8) -> Ioctl<*mut input_absinfo> {
    _IOR(b'E', 0x40 + abs)
}
pub const fn EVIOCSABS(abs: u8) -> Ioctl<*const input_absinfo> {
    _IOW(b'E', 0xc0 + abs)
}

#[cfg(target_os = "freebsd")]
const fn _IOWINT(group: u8, nr: u8) -> Ioctl<c_int> {
    use linux_ioctl::IOC_VOID;

    _IOC(IOC_VOID, group, nr, size_of::<c_int>())
}

/// Send a force feedback effect.
///
/// Takes a mutable pointer because:
///
/// > “request” must be EVIOCSFF.
/// >
/// > “effect” points to a structure describing the effect to upload. The effect is uploaded, but
/// > not played. The content of effect may be modified.
///
/// <https://www.kernel.org/doc/html/latest/input/ff.html>
pub const EVIOCSFF: Ioctl<*mut ff_effect> = _IOW(b'E', 0x80).cast_mut();
/// Erase a force feedback effect.
#[cfg(not(target_os = "freebsd"))]
pub const EVIOCRMFF: Ioctl<c_int> = _IOW(b'E', 0x81).with_direct_arg();
#[cfg(target_os = "freebsd")]
pub const EVIOCRMFF: Ioctl<c_int> = _IOWINT(b'E', 0x81);
/// Report the number of FF effects that can play simultaneously.
pub const EVIOCGEFFECTS: Ioctl<*mut c_int> = _IOR(b'E', 0x84);

/// Grab/Release device.
#[cfg(not(target_os = "freebsd"))]
pub const EVIOCGRAB: Ioctl<c_int> = _IOW(b'E', 0x90).with_direct_arg();
#[cfg(target_os = "freebsd")]
pub const EVIOCGRAB: Ioctl<c_int> = _IOWINT(b'E', 0x90);
/// Revoke device access.
#[cfg(not(target_os = "freebsd"))]
pub const EVIOCREVOKE: Ioctl<c_int> = _IOW(b'E', 0x91).with_direct_arg();
#[cfg(target_os = "freebsd")]
pub const EVIOCREVOKE: Ioctl<c_int> = _IOWINT(b'E', 0x91);

/// Get event mask.
pub const EVIOCGMASK: Ioctl<*mut input_mask> = _IOR(b'E', 0x92);
/// Set event mask.
pub const EVIOCSMASK: Ioctl<*const input_mask> = _IOW(b'E', 0x93);

// Somehow *this* one takes the `int` argument indirectly...
pub const EVIOCSCLOCKID: Ioctl<*const c_int> = _IOW(b'E', 0xa0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_replay {
    pub length: u16,
    pub delay: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_trigger {
    pub button: u16,
    pub interval: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_envelope {
    pub attack_length: u16,
    pub attack_level: u16,
    pub fade_length: u16,
    pub fade_level: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_constant_effect {
    pub level: i16,
    pub envelope: ff_envelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_ramp_effect {
    pub start_level: i16,
    pub end_level: i16,
    pub envelope: ff_envelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_condition_effect {
    pub right_saturation: u16,
    pub left_saturation: u16,

    pub right_coeff: i16,
    pub left_coeff: i16,

    pub deadband: u16,
    pub center: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_periodic_effect {
    pub waveform: u16,
    pub period: u16,
    pub magnitude: i16,
    pub offset: i16,
    pub phase: u16,

    pub envelope: ff_envelope,

    pub custom_len: u32,
    pub custom_data: *mut i16,
}
unsafe impl Send for ff_periodic_effect {}
unsafe impl Sync for ff_periodic_effect {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ff_rumble_effect {
    pub strong_magnitude: u16,
    pub weak_magnitude: u16,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ff_effect {
    pub type_: u16,
    pub id: i16,
    pub direction: u16,
    pub trigger: ff_trigger,
    pub replay: ff_replay,

    pub u: ff_effect_union,
}

#[derive(Clone, Copy)]
pub union ff_effect_union {
    pub constant: ff_constant_effect,
    pub ramp: ff_ramp_effect,
    pub periodic: ff_periodic_effect,
    pub condition: [ff_condition_effect; 2],
    pub rumble: ff_rumble_effect,
}
