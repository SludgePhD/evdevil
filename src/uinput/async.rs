#![cfg(any(feature = "tokio", feature = "async-io"))]

use core::task::Poll;
use std::{io, os::unix::prelude::AsRawFd};

use crate::{event::InputEvent, uinput::UinputDevice, util::r#async::AsyncHelper};

/// An asynchronous iterator over [`InputEvent`]s produced by an [`UinputDevice`].
///
/// Returned by [`UinputDevice::async_events`].
///
/// Note that this type does not yet implement the `AsyncIterator` trait, since that is still
/// unstable.
/// To fetch [`InputEvent`]s, use [`AsyncEvents::next_event`].
#[derive(Debug)]
pub struct AsyncEvents<'a> {
    helper: AsyncHelper,
    dev: &'a UinputDevice,
}

impl<'a> AsyncEvents<'a> {
    pub(crate) fn new(dev: &'a UinputDevice) -> io::Result<Self> {
        Ok(Self {
            helper: AsyncHelper::new(dev.as_raw_fd())?,
            dev,
        })
    }

    /// Asynchronously fetches the next [`InputEvent`] from the [`UinputDevice`].
    ///
    /// When using the `"tokio"` feature, this method must be called from within a tokio context.
    pub async fn next_event(&mut self) -> io::Result<InputEvent> {
        self.helper
            .asyncify(|| match self.dev.events().next() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        event::{EventKind, Led, LedEvent},
        test::pair,
        util::r#async::test::AsyncTest,
    };

    use super::*;

    #[test]
    #[cfg_attr(
        target_os = "freebsd",
        ignore = "FreeBSD does not support non-blocking uinput devices"
    )]
    fn smoke() -> io::Result<()> {
        let (uinput, evdev) = pair(|b| b.with_leds([Led::CAPSL]))?;

        AsyncTest::new(
            async {
                let ev = uinput.async_events()?.next_event().await?;
                match ev.kind() {
                    EventKind::Led(ev) if ev.led() == Led::CAPSL && ev.is_on() => {}
                    _ => panic!("unexpected event {ev:?}"),
                }
                Ok(())
            },
            || evdev.write(&[LedEvent::new(Led::CAPSL, true).into()]),
        )
        .run()?;

        Ok(())
    }
}
