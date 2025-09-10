//! Support for hotplug events.
//!
//! The recommended way to support device hotplug in applications is to use the
//! [`enumerate_hotplug`] function, which returns an iterator over all devices that are or will be
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
//! [`enumerate_hotplug`]: crate::enumerate_hotplug

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
    path::{Path, PathBuf},
};

use crate::{Evdev, util::set_nonblocking};

trait HotplugImpl: Sized + AsRawFd + IntoRawFd {
    fn open() -> io::Result<Self>;
    fn read(&self) -> io::Result<HotplugEvent>;
}

/// Monitors the system for newly plugged in input devices.
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
    /// [`Iter::next`] and [`IntoIter::next`] will return [`None`] when the socket is in
    /// non-blocking mode and there are no incoming hotplug events.
    ///
    /// Note that the act of opening a device is always blocking, and may block for a significant
    /// amount of time, so "non-blocking" operation only covers generation of [`HotplugEvent`]s,
    /// not opening the device the events refer to.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<bool> {
        set_nonblocking(self.as_raw_fd(), nonblocking)
    }

    /// Returns an iterator that yields hotplug events.
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter(self)
    }
}

impl IntoIterator for HotplugMonitor {
    type Item = io::Result<HotplugEvent>;
    type IntoIter = IntoIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self)
    }
}

impl<'a> IntoIterator for &'a HotplugMonitor {
    type Item = io::Result<HotplugEvent>;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        Iter(self)
    }
}

/// An owning [`Iterator`] over hotplug events.
///
/// Returned by [`HotplugMonitor::into_iter`].
///
/// If [`HotplugMonitor::set_nonblocking`] has been used to put the [`HotplugMonitor`] in
/// non-blocking mode, this iterator will yield [`None`] when no events are pending.
/// Otherwise, it will block until a hotplug event arrives.
#[derive(Debug)]
pub struct IntoIter(HotplugMonitor);

impl Iterator for IntoIter {
    type Item = io::Result<HotplugEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.imp.read() {
            Ok(ev) => Some(Ok(ev)),
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
    type Item = io::Result<HotplugEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.imp.read() {
            Ok(ev) => Some(Ok(ev)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// An event emitted by the [`HotplugMonitor`].
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    path: PathBuf,
}

impl HotplugEvent {
    /// Returns the device path indicated by this event.
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens the [`Evdev`] indicated by this event.
    ///
    /// This operation is always blocking, and may block for a significant amount of time.
    pub fn open(&self) -> io::Result<Evdev> {
        Evdev::open(&self.path)
    }
}
