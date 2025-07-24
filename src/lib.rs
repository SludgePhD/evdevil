//! Rust bindings to `evdev` and `uinput`.
//!
//! This crate provides an ergonomic interface to Linux' input subsystem.

#![warn(missing_debug_implementations)]

#[macro_use]
mod macros;

#[cfg(test)]
mod test;

mod abs_info;
mod batch;
pub mod bits;
mod drop;
pub mod enumerate;
mod evdev;
pub mod event;
pub mod ff;
pub mod hotplug;
mod input_id;
mod key_repeat;
mod keymap_entry;
mod raw;
pub mod reader;
mod slot;
pub mod uinput;
mod util;
mod version;

pub use abs_info::AbsInfo;
#[doc(inline)]
pub use enumerate::{enumerate, enumerate_hotplug};
pub use evdev::*;
pub use event::codes::{InputProp, UnknownVariant};
pub use input_id::{Bus, InputId};
pub use key_repeat::KeyRepeat;
pub use keymap_entry::{KeymapEntry, Scancode};
#[doc(inline)]
pub use reader::EventReader;
pub use slot::Slot;
pub use version::Version;

#[doc = include_str!("../README.md")]
mod readme {}

#[cfg(test)]
mod tests {
    use crate::{hotplug::HotplugMonitor, uinput::UinputDevice};

    use super::*;

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync>() {}

        assert::<Evdev>();
        assert::<EventReader>();
        assert::<UinputDevice>();
        assert::<HotplugMonitor>();
    }
}
