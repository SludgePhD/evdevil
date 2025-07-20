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
        io,
        os::fd::{BorrowedFd, RawFd},
        task::Poll,
    };

    use async_io::Async;

    #[derive(Debug)]
    pub struct Impl(Async<BorrowedFd<'static>>);

    impl Impl {
        pub fn new(fd: RawFd) -> io::Result<Self> {
            let fd = unsafe { BorrowedFd::borrow_raw(fd) };
            Async::new(fd).map(Self)
        }

        pub async fn asyncify<T>(
            &self,
            mut op: impl FnMut() -> Poll<io::Result<T>>,
        ) -> io::Result<T> {
            loop {
                match op() {
                    Poll::Pending => self.0.readable().await?,
                    Poll::Ready(res) => return res,
                }
            }
        }
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

/// Calls `f` with an instance of the selected async runtime.
///
/// Allows writing async-runtime-agnostic tests.
///
/// The only supported API is `runtime.block_on(future)`.
#[cfg(test)]
pub fn with_runtime<R>(f: impl FnOnce(&Runtime) -> io::Result<R>) -> io::Result<R> {
    let rt = Runtime::new()?;
    let _guard = rt.enter();
    f(&rt)
}
