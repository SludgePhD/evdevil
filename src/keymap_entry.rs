use std::{fmt, mem};

use crate::{event::Key, raw::input::input_keymap_entry};

/// A device keymap entry translates a scancode to a keycode.
///
/// **Note**: This is not the same as the *localized* keymap you use in applications (like QWERTZ or
/// AZERTY), which is handled by the X server or Wayland compositor.
/// Instead, this keymap exists to translate raw scancodes from the keyboard to USB-HID keycodes,
/// which are defined as a US layout.
///
/// Returned by [`Evdev::keymap_entry`] and [`Evdev::keymap_entry_by_index`].
///
/// [`Evdev::keymap_entry`]: crate::Evdev::keymap_entry
/// [`Evdev::keymap_entry_by_index`]: crate::Evdev::keymap_entry_by_index
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct KeymapEntry(pub(crate) input_keymap_entry);

impl KeymapEntry {
    #[inline]
    pub(crate) fn zeroed() -> Self {
        unsafe { mem::zeroed() }
    }

    /// Zero-based index of this entry in the keymap.
    #[inline]
    pub fn index(&self) -> u16 {
        self.0.index
    }

    /// Returns the [`Key`] associated with this scancode.
    #[inline]
    pub fn keycode(&self) -> Key {
        let key = self.0.keycode as u16;
        Key::from_raw(key)
    }

    /// Returns the [`Scancode`] of this entry.
    ///
    /// If the key that produces this [`Scancode`] is pressed, the [`Key`] returned by
    /// [`KeymapEntry::keycode`] will be generated.
    /// The OS will then typically translate that [`Key`] to the configured localized keyboard
    /// layout.
    #[inline]
    pub fn scancode(&self) -> Scancode {
        let len = self.0.len.min(32);
        Scancode::from_ne_slice(&self.0.scancode[..len as usize])
    }
}

impl fmt::Debug for KeymapEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeymapEntry")
            .field("index", &self.index())
            .field("keycode", &self.keycode())
            .field("scancode", &self.scancode())
            .finish()
    }
}

/*

The Scripture states:

switch (ke->len) {
case 1:
    *scancode = *((u8 *)ke->scancode);
    break;

case 2:
    *scancode = *((u16 *)ke->scancode);
    break;

case 4:
    *scancode = *((u32 *)ke->scancode);
    break;

default:
    return -EINVAL;
}

Therefore we have to make sure that we only use lengths that have the divine blessing.

*/

/// A raw scancode emitted by a keyboard.
///
/// Can be constructed from raw integers via [`From<u8>`], [`From<u16>`] and [`From<u32>`].
///
/// Each [`KeymapEntry`] maps a scancode to an evdev key code.
/// The [`Scancode`] belonging to an entry can be fetched with [`KeymapEntry::scancode`].
///
/// The [`Scancode`] type is also used when modifying the keymap using the
/// [`Evdev::set_keymap_entry`] and [`Evdev::set_keymap_entry_by_index`] functions.
///
/// [`Evdev::set_keymap_entry`]: crate::Evdev::set_keymap_entry
/// [`Evdev::set_keymap_entry_by_index`]: crate::Evdev::set_keymap_entry_by_index
#[derive(Clone, Copy)]
pub struct Scancode {
    // NOTE: not currently canonicalized; there may be leading zeroes
    pub(crate) len: u8,
    // In native byte order.
    pub(crate) bytes: [u8; 32],
}

impl Scancode {
    fn from_ne_slice(bytes: &[u8]) -> Self {
        assert!(bytes.len() <= 32);
        let mut a = [0; 32];
        a[..bytes.len()].copy_from_slice(&bytes);
        Self {
            len: bytes.len() as u8,
            bytes: a,
        }
    }

    fn as_ne_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    fn iter_ne_bytes(&self) -> impl DoubleEndedIterator<Item = u8> {
        self.as_ne_bytes().iter().copied()
    }
    fn iter_be_bytes(&self) -> impl Iterator<Item = u8> {
        #[cfg(target_endian = "little")]
        return self.iter_ne_bytes().rev();

        #[cfg(target_endian = "big")]
        return self.iter_ne_bytes();
    }
}

impl From<u8> for Scancode {
    #[inline]
    fn from(value: u8) -> Self {
        Self::from_ne_slice(&[value])
    }
}
impl From<u16> for Scancode {
    #[inline]
    fn from(value: u16) -> Self {
        Self::from_ne_slice(&value.to_ne_bytes())
    }
}
impl From<u32> for Scancode {
    #[inline]
    fn from(value: u32) -> Self {
        Self::from_ne_slice(&value.to_ne_bytes())
    }
}

impl fmt::Debug for Scancode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut any_nonzero = false;
        for (i, byte) in self.iter_be_bytes().skip_while(|b| *b == 0).enumerate() {
            if i == 0 {
                write!(f, "{byte:x}")?;
            } else {
                write!(f, "{byte:02x}")?;
            }
            any_nonzero = true;
        }

        if !any_nonzero {
            f.write_str("0")?;
        }

        Ok(())
    }
}
impl fmt::Display for Scancode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        <Self as fmt::Debug>::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let code = Scancode::from(0u8);
        assert_eq!(format!("{code}"), "0");

        let code = Scancode::from(1u8);
        assert_eq!(format!("{code}"), "1");

        let code = Scancode::from(10u8);
        assert_eq!(format!("{code}"), "a");

        let code = Scancode::from(0x10u8);
        assert_eq!(format!("{code}"), "10");

        let code = Scancode::from(0x100u16);
        assert_eq!(format!("{code}"), "100");

        let code = Scancode::from(0x1000u16);
        assert_eq!(format!("{code}"), "1000");
    }
}
