use std::{io, os::fd::AsRawFd, task::Poll};

use crate::{Evdev, hotplug::HotplugMonitor, util::r#async::AsyncHelper};

/// An asynchronous iterator over hotplug events.
///
/// Returned by [`HotplugMonitor::async_iter`].
#[derive(Debug)]
pub struct AsyncIter<'a> {
    helper: AsyncHelper,
    mon: &'a HotplugMonitor,
}

impl<'a> AsyncIter<'a> {
    pub(crate) fn new(mon: &'a HotplugMonitor) -> io::Result<Self> {
        Ok(Self {
            helper: AsyncHelper::new(mon.as_raw_fd())?,
            mon,
        })
    }

    /// Asynchronously waits for the next hotplug event.
    pub async fn next_event(&self) -> io::Result<Evdev> {
        self.helper
            .asyncify(|| match self.mon.iter().next() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::{hotplug::HotplugMonitor, uinput::UinputDevice, util::r#async::test::AsyncTest};

    #[test]
    fn smoke() -> io::Result<()> {
        env_logger::try_init().ok();

        const DEVNAME: &str = "-@-rust-async-hotplug-test-@-";

        let mon = HotplugMonitor::new()?;

        let mut uinput = None;
        let fut = async {
            // Wait for our test device to arrive:
            loop {
                if let Ok(evdev) = mon.async_iter()?.next_event().await {
                    if let Ok(name) = evdev.name() {
                        if name == DEVNAME {
                            return Ok(evdev);
                        }
                    }
                }
            }
        };
        AsyncTest::new(fut, || {
            uinput = Some(UinputDevice::builder()?.build(DEVNAME)?);
            println!("unblocked");
            Ok(())
        })
        // This test might take a few tries since unrelated events need to be filtered out, and
        // unrelated messages may arrive at the socket, causing a wakeup that results in `Pending`.
        .allowed_polls(1024)
        .run()?;

        Ok(())
    }
}
