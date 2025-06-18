use std::{ffi::c_int, fmt};

/// An `evdev` subsystem version.
///
/// This is the version of the `evdev` input system core, not the version of a device-specific
/// driver.
///
/// Returned by [`Evdev::driver_version`][crate::Evdev::driver_version].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Version(pub(crate) c_int);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for part in self.0.to_be_bytes().into_iter().skip_while(|b| *b == 0) {
            if !first {
                f.write_str(".")?;
            }
            first = false;
            write!(f, "{part}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Version")
            .field(&format!("{:#x}", self.0))
            .finish()
    }
}
