#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations)]
#![cfg_attr(docsrs, feature(doc_cfg))]

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

use std::{
    fs::File,
    io::{self, Read},
    slice,
};

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

use crate::event::InputEvent;

/// Reads raw events from an `Evdev` or a `UinputDevice` into `dest`.
///
/// Returns the number of events that were read.
fn read_raw(mut file: &File, dest: &mut [InputEvent]) -> io::Result<usize> {
    let bptr = dest.as_mut_ptr().cast::<u8>();
    // Safety: this requires that `InputEvent` contains no padding, which is tested where `input_event` is defined.
    let byte_buf = unsafe { slice::from_raw_parts_mut(bptr, size_of::<InputEvent>() * dest.len()) };
    let bytes = file.read(byte_buf)?;
    debug_assert_eq!(bytes % size_of::<InputEvent>(), 0);
    Ok(bytes / size_of::<InputEvent>())
}

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
