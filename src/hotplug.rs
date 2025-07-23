//! Support for hotplug events.
//!
//! The recommended way to support device hotplug in applications is to use the
//! [`hotplug::enumerate`] function, which returns an iterator over all devices that are or will be
//! plugged into the system.
//!
//! # Platform Support
//!
//! Hotplug functionality is supported on Linux and FreeBSD, as follows:
//!
//! |   OS    | Details |
//! |---------|---------|
//! | Linux   | Uses the `NETLINK_KOBJECT_UEVENT` socket. Requires `udev`. |
//! | FreeBSD | Uses `devd`'s seqpacket socket at `/var/run/devd.seqpacket.pipe`. |
//!
//! [`hotplug::enumerate`]: crate::hotplug::enumerate

#[cfg_attr(docsrs, doc(cfg(feature = "tokio", feature = "async-io")))]
#[cfg(any(doc, feature = "tokio", feature = "async-io"))]
mod r#async;

#[cfg(any(doc, feature = "tokio", feature = "async-io"))]
pub use r#async::AsyncIter;

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
    os::{
        fd::{AsFd, AsRawFd, IntoRawFd, RawFd},
        unix::prelude::BorrowedFd,
    },
};

use crate::{Evdev, util::set_nonblocking};

trait HotplugImpl: Sized + AsRawFd + IntoRawFd {
    fn open() -> io::Result<Self>;
    fn read(&self) -> io::Result<Evdev>;
}

/// Monitors the system for newly plugged in input devices.
///
/// This type implements [`Iterator`], which will block until the next event is received.
///
/// Iterating over the hotplug events will yield [`io::Result`]s that may be arbitrary
/// [`io::Error`]s that occurred while attempting to open a device.
/// These error may happen at any point, since devices may be removed anytime (resulting in a
/// [`NotFound`][io::ErrorKind::NotFound] error or some other error).
/// Applications should handle these errors non-fatally.
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

impl IntoRawFd for HotplugMonitor {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        self.imp.into_raw_fd()
    }
}

impl AsFd for HotplugMonitor {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

impl HotplugMonitor {
    /// Creates a new [`HotplugMonitor`] and starts listening for hotplug events.
    ///
    /// This operation is always blocking.
    ///
    /// # Errors
    ///
    /// This will fail with [`io::ErrorKind::Unsupported`] on unsupported platforms.
    /// It may also fail with other types of errors if connecting to the system's hotplug mechanism
    /// fails.
    ///
    /// Callers should degrade gracefully, by using only the currently plugged-in devices and not
    /// supporting hotplug functionality.
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

    /// Returns an iterator that yields hotplug events.
    pub fn iter(&self) -> Iter<'_> {
        Iter(self)
    }

    /// Returns an asynchronous iterator that yields hotplug events.
    ///
    /// Requires either the `"tokio"` or the `"async-io"` feature to be enabled.
    ///
    /// The [`HotplugMonitor`] will be put in non-blocking mode while the [`AsyncIter`] is alive
    /// (if it isn't already).
    ///
    /// When using the `"tokio"` Cargo feature, this must be called while inside a tokio context.
    #[cfg_attr(docsrs, doc(cfg(any(doc, feature = "tokio", feature = "async-io"))))]
    #[cfg(any(doc, feature = "tokio", feature = "async-io"))]
    pub fn async_iter(&self) -> io::Result<AsyncIter<'_>> {
        AsyncIter::new(self)
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

/// An [`Iterator`] over hotplug events.
///
/// Returned by [`HotplugMonitor::iter`].
///
/// If [`HotplugMonitor::set_nonblocking`] has been used to put the [`HotplugMonitor`] in
/// non-blocking mode, this iterator will yield [`None`] when no events are pending.
/// Otherwise, it will block until a hotplug event arrives.
#[derive(Debug)]
pub struct Iter<'a>(&'a HotplugMonitor);

impl<'a> Iterator for Iter<'a> {
    type Item = io::Result<Evdev>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.imp.read() {
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
