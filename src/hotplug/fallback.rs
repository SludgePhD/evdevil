use std::{
    io,
    os::{
        fd::{AsRawFd, IntoRawFd},
        unix::prelude::RawFd,
    },
};

use super::{HotplugEvent, HotplugImpl};

#[allow(dead_code)]
pub struct Impl {
    _p: (),
}

impl AsRawFd for Impl {
    fn as_raw_fd(&self) -> RawFd {
        unreachable!("this type cannot be constructed")
    }
}

impl IntoRawFd for Impl {
    fn into_raw_fd(self) -> RawFd {
        unreachable!("this type cannot be constructed")
    }
}

impl HotplugImpl for Impl {
    fn open() -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn read(&self) -> io::Result<HotplugEvent> {
        unreachable!("this type cannot be constructed")
    }
}
