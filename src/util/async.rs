#![cfg(any(doc, feature = "tokio", feature = "async-io"))]

use std::{io, os::fd::RawFd, task::Poll};

use crate::util::set_nonblocking;

/// A helper that makes an operation `async`.
///
/// The target file descriptor is moved into non-blocking mode when the `AsyncHelper` is created,
/// and back out when it is dropped (unless the caller has already put it in non-blocking mode).
///
/// The `asyncify` method can then be used to integrate into an async runtime.
#[derive(Debug)]
pub struct AsyncHelper {
    fd: RawFd,
    was_nonblocking: bool,
    imp: Impl,
}

impl AsyncHelper {
    pub fn new(fd: RawFd) -> io::Result<Self> {
        let was_nonblocking = set_nonblocking(fd, true)?;
        Ok(Self {
            fd,
            was_nonblocking,
            imp: Impl::new(fd)?,
        })
    }

    /// Turns an operation `async`.
    ///
    /// `op` must return `Poll::Pending` when the underlying read fails with `WouldBlock`, and
    /// `Poll::Ready` when a result is available.
    /// `AsyncHelper` will handle the rest of the job (such as registering the fd with the selected
    /// async backend, and waiting until the fd is readable again).
    pub async fn asyncify<T>(&self, op: impl FnMut() -> Poll<io::Result<T>>) -> io::Result<T> {
        self.imp.asyncify(op).await
    }
}

impl Drop for AsyncHelper {
    fn drop(&mut self) {
        if self.was_nonblocking {
            return;
        }

        if let Err(e) = set_nonblocking(self.fd, false) {
            log::error!("failed to move fd back into blocking mode: {e}");
        }
    }
}

#[cfg(feature = "tokio")]
use tokio_impl::*;
#[cfg(feature = "tokio")]
mod tokio_impl {
    use std::{io, os::fd::RawFd, task::Poll};

    use tokio::io::{Interest, unix::AsyncFd};

    #[derive(Debug)]
    pub struct Impl(AsyncFd<RawFd>);

    impl Impl {
        pub fn new(fd: RawFd) -> io::Result<Self> {
            // Note: only register with READABLE interest; otherwise this fails with EINVAL on FreeBSD.
            let fd = AsyncFd::with_interest(fd, Interest::READABLE)?;
            Ok(Self(fd))
        }

        pub async fn asyncify<T>(
            &self,
            mut op: impl FnMut() -> Poll<io::Result<T>>,
        ) -> io::Result<T> {
            let mut guard = None;
            loop {
                match op() {
                    Poll::Pending => guard = Some(self.0.readable().await?),
                    Poll::Ready(res) => {
                        if let Some(mut guard) = guard {
                            guard.clear_ready();
                        }
                        return res;
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub struct Runtime {
        rt: tokio::runtime::Runtime,
    }

    #[cfg(test)]
    impl Runtime {
        pub fn new() -> io::Result<Self> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()?;
            Ok(Self { rt })
        }

        pub fn enter(&self) -> impl Sized + '_ {
            self.rt.enter()
        }

        pub fn block_on<F: Future>(&self, fut: F) -> F::Output {
            self.rt.block_on(fut)
        }
    }
}

#[cfg(feature = "async-io")]
use asyncio_impl::*;
#[cfg(feature = "async-io")]
mod asyncio_impl {
    use std::{
        future, io,
        os::fd::{BorrowedFd, RawFd},
        pin::pin,
        task::Poll,
    };

    use async_io::Async;

    #[derive(Debug)]
    pub struct Impl(Async<BorrowedFd<'static>>);

    impl Impl {
        pub fn new(fd: RawFd) -> io::Result<Self> {
            let fd = unsafe { BorrowedFd::borrow_raw(fd) };
            Async::new_nonblocking(fd).map(Self)
        }

        pub async fn asyncify<T>(
            &self,
            mut op: impl FnMut() -> Poll<io::Result<T>>,
        ) -> io::Result<T> {
            loop {
                match op() {
                    Poll::Pending => optimistic(self.0.readable()).await?,
                    Poll::Ready(res) => return res,
                }
            }
        }
    }

    // This "optimization" is copied from async-io.
    // async-io is apparently very buggy (see smol-rs/async-io#78), so it ends up being required for
    // things to work right.
    // Specifically, the `.readable()` future is permanently `Pending`, even after the reactor
    // schedules the future again, so `asyncify` would just never complete.
    async fn optimistic(fut: impl Future<Output = io::Result<()>>) -> io::Result<()> {
        let mut polled = false;
        let mut fut = pin!(fut);

        future::poll_fn(|cx| {
            if !polled {
                polled = true;
                fut.as_mut().poll(cx)
            } else {
                Poll::Ready(Ok(()))
            }
        })
        .await
    }

    #[cfg(test)]
    pub struct Runtime;

    #[cfg(test)]
    impl Runtime {
        pub fn new() -> io::Result<Self> {
            Ok(Self)
        }

        pub fn enter(&self) -> impl Sized + '_ {}

        pub fn block_on<F: Future>(&self, fut: F) -> F::Output {
            async_io::block_on(fut)
        }
    }
}

// These definitions override the glob-imported ones above and make the documentation build with
// `--all-features`.
#[cfg(doc)]
pub struct Impl;
#[cfg(doc)]
pub struct Runtime;

#[cfg(test)]
pub mod test {
    use std::{fmt, future, panic::resume_unwind, pin::pin, sync::mpsc, thread};

    use super::*;

    pub struct AsyncTest<F, U> {
        future: F,
        unblocker: U,
        allowed_polls: usize,
    }

    impl<F, U> AsyncTest<F, U> {
        pub fn new(future: F, unblocker: U) -> Self {
            Self {
                future,
                unblocker,
                allowed_polls: 1,
            }
        }

        /// Sets the number of allowed future polls after the `unblocker` has been run.
        ///
        /// By default, this is 1, expecting the future to complete immediately after the waker has
        /// been notified.
        /// Higher values may be needed if the API-under-test is system-global and may have to
        /// process some irrelevant events until it becomes `Ready`.
        pub fn allowed_polls(mut self, allowed_polls: usize) -> Self {
            self.allowed_polls = allowed_polls;
            self
        }

        /// Polls `future`, expecting `Poll::Pending`. Then runs `unblocker`, and expects the waker to
        /// be invoked and the `future` to be `Poll::Ready`.
        pub fn run<T>(self) -> io::Result<T>
        where
            F: Future<Output = io::Result<T>> + Send,
            F::Output: Send,
            U: FnOnce() -> io::Result<()>,
            T: fmt::Debug,
        {
            let (sender, recv) = mpsc::sync_channel(0);
            thread::scope(|s| {
                let h = s.spawn(move || -> io::Result<_> {
                    let rt = Runtime::new()?;
                    let _guard = rt.enter();
                    let mut fut = pin!(self.future);
                    let mut poll_count = 0;

                    rt.block_on(future::poll_fn(|cx| {
                        if poll_count == 0 {
                            match fut.as_mut().poll(cx) {
                                Poll::Ready(val) => {
                                    panic!("expected future to be `Pending`, but it is `Ready({val:?})`")
                                }
                                Poll::Pending => {
                                    // Waker is now scheduled to be woken when the event of interest occurs.
                                    println!("future is pending; scheduling wakeup");
                                    poll_count += 1;
                                    sender.send(()).unwrap();
                                    return Poll::Pending;
                                }
                            }
                        } else {
                            // This is called when the `Waker` has been woken up.
                            match fut.as_mut().poll(cx) {
                                Poll::Ready(out) => Poll::Ready(out),
                                Poll::Pending => {
                                    if poll_count >= self.allowed_polls {
                                        panic!("future still `Pending` after {poll_count} polls");
                                    }
                                    poll_count += 1;
                                    Poll::Pending
                                }
                            }
                        }
                    }))
                });

                recv.recv().unwrap();

                // We've been signaled to invoke `unblocker`.
                (self.unblocker)()?;

                match h.join() {
                    Ok(res) => res,
                    Err(payload) => resume_unwind(payload),
                }
            })
        }
    }
}
