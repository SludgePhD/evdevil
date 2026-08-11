use std::{
    io,
    os::unix::prelude::{AsFd, BorrowedFd, OwnedFd},
};

use super::{HotplugEvent, HotplugImpl};

#[allow(dead_code)]
pub enum Impl {}

impl HotplugImpl for Impl {
    #[inline]
    fn open() -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    #[inline]
    fn read(&self) -> io::Result<HotplugEvent> {
        match *self {}
    }
}

impl AsFd for Impl {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        match *self {}
    }
}

impl Into<OwnedFd> for Impl {
    #[inline]
    fn into(self) -> OwnedFd {
        match self {}
    }
}
