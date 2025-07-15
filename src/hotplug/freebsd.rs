use std::{
    ffi::{OsStr, c_int},
    io,
    mem::{self, offset_of},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::PathBuf,
};

use libc::{AF_UNIX, SOCK_CLOEXEC, SOCK_SEQPACKET, connect, recv, sockaddr_un, socket, ssize_t};

use crate::Evdev;

fn cvt(ret: c_int) -> io::Result<c_int /* never -1 */> {
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

fn cvt_r(mut f: impl FnMut() -> ssize_t) -> io::Result<ssize_t> {
    loop {
        let ret = f();
        if ret == -1 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        } else {
            return Ok(ret);
        }
    }
}

pub(crate) struct Impl {
    fd: OwnedFd,
}

impl AsRawFd for Impl {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl super::HotplugImpl for Impl {
    fn open() -> io::Result<Self> {
        const PATH: &[u8] = b"/var/run/devd.seqpacket.pipe";

        unsafe {
            let fd = OwnedFd::from_raw_fd(cvt(socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0))?);
            let mut addr: sockaddr_un = mem::zeroed();
            addr.sun_path
                .as_mut_ptr()
                .cast::<u8>()
                .copy_from_nonoverlapping(PATH.as_ptr(), PATH.len());
            addr.sun_len = (offset_of!(sockaddr_un, sun_path) + PATH.len())
                .try_into()
                .unwrap();
            addr.sun_family = AF_UNIX as _;

            cvt(connect(
                fd.as_raw_fd(),
                (&raw const addr).cast(),
                mem::size_of_val(&addr) as _,
            ))?;

            Ok(Self { fd })
        }
    }

    fn read(&self) -> io::Result<Evdev> {
        let mut buf = [0u8; 8192];
        loop {
            unsafe {
                let len =
                    cvt_r(|| recv(self.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len() as _, 0))?;
                let msg = &buf[..len as usize];

                // The messages we're looking for are newline-terminated and look like this:
                // !system=DEVFS subsystem=CDEV type=CREATE cdev=input/eventN
                let Some(msg) = msg.strip_prefix(b"!") else {
                    continue;
                };
                let Some(msg) = msg.strip_suffix(b"\n") else {
                    continue;
                };

                log::trace!("incoming devd message: {}", msg.escape_ascii());

                let mut system_devfs = false;
                let mut subsys_cdev = false;
                let mut type_create = false;
                let mut cdev = None;
                for part in msg.split(|b| *b == b' ') {
                    let mut split = part.splitn(2, |b| *b == b'=');
                    let Some(key) = split.next() else {
                        continue;
                    };
                    let Some(value) = split.next() else {
                        continue;
                    };

                    match key {
                        b"system" if value == b"DEVFS" => system_devfs = true,
                        b"subsystem" if value == b"CDEV" => subsys_cdev = true,
                        b"type" if value == b"CREATE" => type_create = true,
                        b"cdev" => cdev = Some(value),
                        _ => {}
                    }
                }

                if system_devfs && subsys_cdev && type_create {
                    if let Some(cdev) = cdev {
                        let mut path = PathBuf::from("/dev/");
                        path.push(OsStr::from_bytes(cdev));

                        log::debug!("match! trying to open: {}", path.display());
                        return Evdev::open(path);
                    }
                }

                log::trace!("no match");
            }
        }
    }
}
