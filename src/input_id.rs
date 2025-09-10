use std::fmt::{self, LowerHex};

use crate::raw::input::input_id;

/// Input device ID.
///
/// `uinput` devices, devices exported by ALSA, and other devices often leave this structure empty
/// (all-zeroes).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct InputId(pub(crate) input_id);

impl InputId {
    /// Creates an [`InputId`] from its components.
    #[inline]
    pub const fn new(bus: Bus, vendor: u16, product: u16, version: u16) -> Self {
        Self(input_id {
            bustype: bus.0,
            vendor,
            product,
            version,
        })
    }

    /// Returns the bus type this device is attached to the system with.
    ///
    /// This is often left as `0` for virtual devices.
    #[inline]
    pub fn bus(&self) -> Bus {
        Bus(self.0.bustype)
    }

    /// Returns the vendor ID.
    ///
    /// For USB and PCI devices, the vendor ID is typically taken from the device descriptor and may
    /// be looked up in the corresponding registry.
    #[inline]
    pub fn vendor(&self) -> u16 {
        self.0.vendor
    }

    /// Returns the product ID.
    ///
    /// For USB and PCI devices, the product ID is typically taken from the device descriptor and may
    /// be looked up in the corresponding registry.
    #[inline]
    pub fn product(&self) -> u16 {
        self.0.product
    }

    /// The device or transport version.
    ///
    /// For USB devices, this is typically an encoding of the implemented USB-HID version
    /// (`bcdHID`).
    #[inline]
    pub fn version(&self) -> u16 {
        self.0.version
    }
}

impl fmt::Debug for InputId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Hex<T: LowerHex>(T);
        impl<T: LowerHex> fmt::Debug for Hex<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:#06x}", self.0)
            }
        }

        f.debug_struct("InputId")
            .field("bustype", &self.bus())
            .field("vendor", &Hex(self.vendor()))
            .field("product", &Hex(self.product()))
            .field("version", &Hex(self.version()))
            .finish()
    }
}

ffi_enum! {
    /// Bus types that devices can be attached to the system with.
    pub enum Bus: u16 {
        PCI         = 0x01,
        ISAPNP      = 0x02,
        USB         = 0x03,
        HIL         = 0x04,
        BLUETOOTH   = 0x05,
        VIRTUAL     = 0x06,
        ISA         = 0x10,
        I8042       = 0x11,
        XTKBD       = 0x12,
        RS232       = 0x13,
        GAMEPORT    = 0x14,
        PARPORT     = 0x15,
        AMIGA       = 0x16,
        ADB         = 0x17,
        I2C         = 0x18,
        HOST        = 0x19,
        GSC         = 0x1A,
        ATARI       = 0x1B,
        SPI         = 0x1C,
        RMI         = 0x1D,
        CEC         = 0x1E,
        INTEL_ISHTP = 0x1F,
        AMD_SFH     = 0x20,
    }
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => write!(f, "BUS_{name}"),
            None => write!(f, "Bus({:#x})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_debug() {
        assert_eq!(format!("{:?}", Bus::USB), "BUS_USB");
        assert_eq!(format!("{:?}", Bus(0xffff)), "Bus(0xffff)");
    }
}
