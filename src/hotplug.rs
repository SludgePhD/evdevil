//! Support for hotplug events.
//!
//! This module is currently Linux-specific. It uses the udev netlink socket to listen for events
//! from a udev implementation.
//!
//! The recommended way to support device hotplug is to use the [`hotplug::enumerate`] function,
//! which returns an iterator over all devices that are or will be plugged into the system.
//!
//! [`hotplug::enumerate`]: crate::hotplug::enumerate

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Impl;

#[cfg(target_os = "freebsd")]
mod freebsd;
#[cfg(target_os = "freebsd")]
use freebsd::Impl;

mod fallback;
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
use fallback::Impl;

use std::{
    fmt, io,
    os::fd::{AsRawFd, RawFd},
};

use crate::{Evdev, util::set_nonblocking};

trait HotplugImpl: Sized + AsRawFd {
    fn open() -> io::Result<Self>;
    fn read(&self) -> io::Result<Evdev>;
}

/// Monitors the system for newly plugged in input devices.
///
/// This type implements [`Iterator`], which will block until the next event is received.
pub struct HotplugMonitor {
    imp: Impl,
}

impl fmt::Debug for HotplugMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HotplugMonitor")
            .field("fd", &self.as_raw_fd())
            .finish()
    }
}

impl AsRawFd for HotplugMonitor {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.imp.as_raw_fd()
    }
}

impl HotplugMonitor {
    /// Creates a new [`HotplugMonitor`] and starts listening for hotplug events.
    ///
    /// # Errors
    ///
    /// This will fail with [`io::ErrorKind::Unsupported`] on unsupported platforms.
    /// Callers should degrade gracefully, by using only the currently plugged-in devices and not
    /// supporting hotplug functionality.
    ///
    /// It may fail with other types of errors if connecting to the system's hotplug mechanism
    /// fails.
    pub fn new() -> io::Result<Self> {
        Ok(Self { imp: Impl::open()? })
    }

    /// Moves the socket into or out of non-blocking mode.
    ///
    /// [`HotplugMonitor::next`] will return [`None`] when the socket is in non-blocking mode and
    /// there are no incoming hotplug events.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<bool> {
        set_nonblocking(self.as_raw_fd(), nonblocking)
    }
}

impl Iterator for HotplugMonitor {
    type Item = io::Result<Evdev>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.imp.read() {
            Ok(dev) => Some(Ok(dev)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Enumerates all `evdev` devices, including hotplugged ones.
///
/// This will first yield all devices currently plugged in, and then starts yielding hotplug events
/// similar to [`HotplugMonitor`].
///
/// This allows an application to process a single stream of [`Evdev`]s to both open an already
/// plugged-in device on startup, but also to react to hot-plugged devices automatically, which is
/// typically the desired UX of applications.
///
/// Like [`crate::enumerate`], this function returns a *blocking* iterator that might take a
/// significant amount of time to open each device.
/// This iterator will also keep blocking as it waits for hotplug events, but might terminate if
/// hotplug events are unavailable.
///
/// If hotplug support is unimplemented on the current platform, this will degrade gracefully and
/// only yield the currently plugged-in devices.
pub fn enumerate() -> io::Result<impl Iterator<Item = io::Result<Evdev>>> {
    let monitor = match HotplugMonitor::new() {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            log::warn!("hotplug is not supported on this platform; hotplugged devices won't work");
            None
        }
        Err(e) => return Err(e),
    };
    Ok(crate::enumerate()?.chain(monitor.into_iter().flatten()))
}
