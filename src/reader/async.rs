#![cfg(any(feature = "tokio", feature = "async-io"))]

#[cfg(all(not(doc), feature = "tokio", feature = "async-io"))]
compile_error!("`tokio` and `async-io` are mutually exclusive; only one may be enabled");

use std::{io, os::fd::AsRawFd, task::Poll};

use crate::{EventReader, event::InputEvent, reader::Report, util::r#async::AsyncHelper};

/// An asynchronous iterator over [`Report`]s emitted by the device.
///
/// Returned by [`EventReader::async_reports`].
///
/// Note that this type does not yet implement the `AsyncIterator` trait, since that is still
/// unstable.
/// To fetch [`Report`]s, use [`AsyncReports::next_report`].
#[derive(Debug)]
pub struct AsyncReports<'a> {
    helper: AsyncHelper,
    reader: &'a mut EventReader,
}

impl<'a> AsyncReports<'a> {
    pub(crate) fn new(reader: &'a mut EventReader) -> io::Result<Self> {
        Ok(Self {
            helper: AsyncHelper::new(reader.as_raw_fd())?,
            reader,
        })
    }

    /// Asynchronously fetches the next [`Report`] from the device.
    ///
    /// When using the `"tokio"` feature, this method must be called from within a tokio context.
    pub async fn next_report(&mut self) -> io::Result<Report> {
        self.helper
            .asyncify(|| match self.reader.reports().next() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            })
            .await
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
    helper: AsyncHelper,
    reader: &'a mut EventReader,
}

impl<'a> AsyncEvents<'a> {
    pub(crate) fn new(reader: &'a mut EventReader) -> io::Result<Self> {
        Ok(Self {
            helper: AsyncHelper::new(reader.as_raw_fd())?,
            reader,
        })
    }

    /// Asynchronously fetches the next [`InputEvent`] from the [`EventReader`].
    ///
    /// When using the `"tokio"` feature, this method must be called from within a tokio context.
    pub async fn next_event(&mut self) -> io::Result<InputEvent> {
        self.helper
            .asyncify(|| match self.reader.events().next() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{io, pin::pin};

    use crate::{
        event::{Rel, RelEvent, Syn},
        test::{AssertPending, check_events, pair},
        util::r#async::with_runtime,
    };

    #[test]
    fn smoke() -> io::Result<()> {
        with_runtime(|rt| {
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
        })
    }
}
