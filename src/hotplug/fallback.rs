use std::{
    io,
    os::{fd::AsRawFd, unix::prelude::RawFd},
};

use crate::Evdev;

use super::HotplugImpl;

#[allow(dead_code)]
pub struct Impl {
    _p: (),
}

impl AsRawFd for Impl {
    fn as_raw_fd(&self) -> RawFd {
        unreachable!("this type cannot be constructed")
    }
}

impl HotplugImpl for Impl {
    fn open() -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn read(&self) -> io::Result<Evdev> {
        unreachable!("this type cannot be constructed")
    }
}
