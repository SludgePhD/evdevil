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
    use std::{io, pin::pin};

    use crate::{
        hotplug::HotplugMonitor, test::AssertPending, uinput::UinputDevice,
        util::r#async::with_runtime,
    };

    #[test]
    fn smoke() -> io::Result<()> {
        with_runtime(|rt| {
            const DEVNAME: &str = "-@-rust-async-hotplug-test-@-";

            let mon = HotplugMonitor::new()?;

            let events = mon.async_iter()?;
            let mut fut = pin!(events.next_event());
            rt.block_on(AssertPending(fut.as_mut()));

            let _uinput = UinputDevice::builder()?.build(DEVNAME)?;

            rt.block_on(fut)?;

            Ok(())
        })
    }
}
