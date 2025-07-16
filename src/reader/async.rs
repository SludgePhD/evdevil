#![cfg(any(feature = "tokio", feature = "async-io"))]

#[cfg(all(not(doc), feature = "tokio", feature = "async-io"))]
compile_error!("`tokio` and `async-io` are mutually exclusive; only one may be enabled");

use std::io;

use crate::{EventReader, event::InputEvent, reader::Report};

/// An asynchronous iterator over [`Report`]s emitted by the device.
///
/// Returned by [`EventReader::async_reports`].
///
/// Note that this type does not yet implement the `AsyncIterator` trait, since that is still
/// unstable.
/// To fetch [`Report`]s, use [`AsyncReports::next_report`].
#[derive(Debug)]
pub struct AsyncReports<'a> {
    was_nonblocking: bool,
    imp: Impl<'a>,
}

impl<'a> Drop for AsyncReports<'a> {
    fn drop(&mut self) {
        if !self.was_nonblocking {
            if let Err(e) = self.imp.reader().evdev().set_nonblocking(false) {
                log::error!(
                    "failed to move `Evdev` back into blocking mode in `ReportStream` destructor: {e}"
                );
            }
        }
    }
}

impl<'a> AsyncReports<'a> {
    pub(crate) fn new(reader: &'a mut EventReader) -> io::Result<Self> {
        let was_nonblocking = reader.evdev().set_nonblocking(true)?;
        Ok(Self {
            was_nonblocking,
            imp: Impl::new(reader)?,
        })
    }

    /// Asynchronously fetches the next [`Report`] from the device.
    ///
    /// When using the `"tokio"` feature, this method must be called from within a tokio context.
    pub async fn next_report(&mut self) -> io::Result<Report> {
        self.imp.next_report().await
    }
}

/// An asynchronous iterator over [`InputEvent`]s produced by an [`EventReader`].
///
/// Returned by [`EventReader::async_events`].
///
/// Note that this type does not yet implement the `AsyncIterator` trait, since that is still
/// unstable.
/// To fetch [`InputEvent`]s, use [`AsyncEvents::next_event`].
#[derive(Debug)]
pub struct AsyncEvents<'a> {
    was_nonblocking: bool,
    imp: Impl<'a>,
}

impl<'a> Drop for AsyncEvents<'a> {
    fn drop(&mut self) {
        if !self.was_nonblocking {
            if let Err(e) = self.imp.reader().evdev().set_nonblocking(false) {
                log::error!(
                    "failed to move `Evdev` back into blocking mode in `EventStream` destructor: {e}"
                );
            }
        }
    }
}

impl<'a> AsyncEvents<'a> {
    pub(crate) fn new(reader: &'a mut EventReader) -> io::Result<Self> {
        let was_nonblocking = reader.evdev().set_nonblocking(true)?;
        Ok(Self {
            was_nonblocking,
            imp: Impl::new(reader)?,
        })
    }

    /// Asynchronously fetches the next [`InputEvent`] from the [`EventReader`].
    ///
    /// When using the `"tokio"` feature, this method must be called from within a tokio context.
    pub async fn next_event(&mut self) -> io::Result<InputEvent> {
        self.imp.next_event().await
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl {
    use std::{
        io,
        os::fd::{AsRawFd, RawFd},
    };

    use tokio::io::{Interest, unix::AsyncFd};

    use crate::{EventReader, event::InputEvent, reader::Report};

    #[derive(Debug)]
    pub struct Impl<'a> {
        fd: AsyncFd<RawFd>,
        reader: &'a mut EventReader,
    }

    impl<'a> Impl<'a> {
        pub fn new(reader: &'a mut EventReader) -> io::Result<Self> {
            // Note: only register with READABLE interest; otherwise this fails with EINVAL on FreeBSD.
            let fd = AsyncFd::with_interest(reader.as_raw_fd(), Interest::READABLE)?;
            Ok(Self { fd, reader })
        }

        pub fn reader(&self) -> &EventReader {
            self.reader
        }

        pub async fn next_event(&mut self) -> io::Result<InputEvent> {
            if let Some(res) = self.reader.events().next() {
                return res;
            }

            loop {
                let mut guard = self.fd.readable().await?;
                match self.reader.events().next() {
                    Some(res) => return res,
                    None => guard.clear_ready(), // `WouldBlock`
                }
            }
        }

        pub async fn next_report(&mut self) -> io::Result<Report> {
            if let Some(res) = self.reader.reports().next() {
                return res;
            }

            loop {
                let mut guard = self.fd.readable().await?;
                match self.reader.reports().next() {
                    Some(res) => return res,
                    None => guard.clear_ready(), // `WouldBlock`
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{io, pin::pin};

        use crate::{
            event::{Rel, RelEvent, Syn},
            test::{AssertPending, check_events, pair},
        };

        #[test]
        fn smoke() -> io::Result<()> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()?;
            let _guard = rt.enter();

            let (uinput, evdev) = pair(|b| b.with_rel_axes([Rel::DIAL]))?;
            let mut reader = evdev.into_reader()?;
            let mut events = reader.async_events()?;

            {
                let mut fut = pin!(events.next_event());
                rt.block_on(AssertPending(fut.as_mut()));

                uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;

                let event = rt.block_on(fut)?;
                check_events([event], [RelEvent::new(Rel::DIAL, 1).into()]);
            }

            drop(events);
            let ev = reader.events().next().unwrap()?;
            check_events([ev], [Syn::REPORT.into()]);

            let mut reports = reader.async_reports()?;
            let mut fut = pin!(reports.next_report());
            rt.block_on(AssertPending(fut.as_mut()));

            uinput.write(&[RelEvent::new(Rel::DIAL, 2).into()])?;

            let report = rt.block_on(fut)?;
            assert_eq!(report.len(), 2);
            check_events(
                report,
                [RelEvent::new(Rel::DIAL, 2).into(), Syn::REPORT.into()],
            );

            Ok(())
        }
    }
}

#[cfg(feature = "tokio")]
use tokio_impl::*;

#[cfg(feature = "async-io")]
mod asyncio_impl {
    use std::io;

    use async_io::Async;

    use crate::{EventReader, event::InputEvent, reader::Report};

    #[derive(Debug)]
    pub struct Impl<'a> {
        reader: Async<&'a mut EventReader>,
    }

    impl<'a> Impl<'a> {
        pub fn new(reader: &'a mut EventReader) -> io::Result<Self> {
            let reader = Async::new(reader)?;
            Ok(Self { reader })
        }

        pub fn reader(&self) -> &EventReader {
            self.reader.get_ref()
        }

        pub async fn next_event(&mut self) -> io::Result<InputEvent> {
            if let Some(res) = unsafe { self.reader.get_mut().events().next() } {
                return res;
            }

            loop {
                self.reader.readable().await?;
                if let Some(res) = unsafe { self.reader.get_mut().events().next() } {
                    return res;
                }
            }
        }

        pub async fn next_report(&mut self) -> io::Result<Report> {
            if let Some(res) = unsafe { self.reader.get_mut().reports().next() } {
                return res;
            }

            loop {
                self.reader.readable().await?;
                if let Some(res) = unsafe { self.reader.get_mut().reports().next() } {
                    return res;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{io, pin::pin};

        use async_io::block_on;

        use crate::{
            event::{Rel, RelEvent, Syn},
            test::{AssertPending, check_events, pair},
        };

        #[test]
        fn smoke() -> io::Result<()> {
            let (uinput, evdev) = pair(|b| b.with_rel_axes([Rel::DIAL]))?;
            let mut reader = evdev.into_reader()?;
            let mut events = reader.async_events()?;

            {
                let mut fut = pin!(events.next_event());
                block_on(AssertPending(fut.as_mut()));

                uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;

                let event = block_on(fut)?;
                check_events([event], [RelEvent::new(Rel::DIAL, 1).into()]);
            }

            drop(events);
            let ev = reader.events().next().unwrap()?;
            check_events([ev], [Syn::REPORT.into()]);

            let mut reports = reader.async_reports()?;
            let mut fut = pin!(reports.next_report());
            block_on(AssertPending(fut.as_mut()));

            uinput.write(&[RelEvent::new(Rel::DIAL, 2).into()])?;

            let report = block_on(fut)?;
            assert_eq!(report.len(), 2);
            check_events(
                report,
                [RelEvent::new(Rel::DIAL, 2).into(), Syn::REPORT.into()],
            );

            Ok(())
        }
    }
}

#[cfg(feature = "async-io")]
use asyncio_impl::*;

#[cfg(doc)]
struct Impl<'a>(&'a ());
