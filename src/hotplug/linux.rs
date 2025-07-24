use std::{
    convert::identity,
    ffi::{OsStr, c_int, c_uint},
    io, mem,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
        unix::{ffi::OsStrExt, prelude::RawFd},
    },
    path::PathBuf,
};

use libc::{
    AF_NETLINK, CMSG_DATA, CMSG_FIRSTHDR, CMSG_SPACE, NETLINK_KOBJECT_UEVENT, SCM_CREDENTIALS,
    SO_PASSCRED, SOCK_CLOEXEC, SOCK_DGRAM, SOL_SOCKET, bind, iovec, msghdr, recvmsg, sa_family_t,
    setsockopt, sockaddr_nl, socket, socklen_t, ssize_t, ucred,
};

use crate::hotplug::HotplugEvent;

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

const UDEV_PROLOG: &[u8; 8] = b"libudev\0";
const UDEV_MONITOR_MAGIC: u32 = 0xfeedcafe_u32.to_be();

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C, packed)]
struct udev_monitor_netlink_header {
    prefix: [u8; 8],
    magic: c_uint,
    header_size: c_uint,
    properties_off: c_uint,
    properties_len: c_uint,
    filter_subsystem_hash: c_uint,
    filter_devtype_hash: c_uint,
    filter_tag_bloom_hi: c_uint,
    filter_tag_bloom_lo: c_uint,
}

#[derive(Clone, Copy)]
enum MonitorNetlinkGroup {
    /// Requires root.
    Kernel = 1,
    /// Requires udev.
    Udev = 2,
}

pub struct Impl {
    netlink_socket: OwnedFd,
}

impl Impl {
    fn open_group(group: MonitorNetlinkGroup) -> io::Result<Self> {
        unsafe {
            let fd = OwnedFd::from_raw_fd(cvt(socket(
                AF_NETLINK,
                SOCK_DGRAM | SOCK_CLOEXEC, // blocking by default
                NETLINK_KOBJECT_UEVENT,
            ))?);

            let mut addr: sockaddr_nl = mem::zeroed();
            addr.nl_family = AF_NETLINK as sa_family_t;
            addr.nl_groups = group as _;
            cvt(bind(
                fd.as_raw_fd(),
                (&raw const addr).cast(),
                size_of_val(&addr) as socklen_t,
            ))?;

            let on: c_int = 1;
            cvt(setsockopt(
                fd.as_raw_fd(),
                SOL_SOCKET,
                SO_PASSCRED,
                (&raw const on).cast(),
                size_of_val(&on) as socklen_t,
            ))?;

            Ok(Self { netlink_socket: fd })
        }
    }
}

impl AsRawFd for Impl {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.netlink_socket.as_raw_fd()
    }
}

impl IntoRawFd for Impl {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        self.netlink_socket.into_raw_fd()
    }
}

impl super::HotplugImpl for Impl {
    fn open() -> io::Result<Self> {
        Self::open_group(MonitorNetlinkGroup::Udev)
    }

    fn read(&self) -> io::Result<HotplugEvent> {
        let mut buf = [0u8; 8192];
        let mut cred_msg = [0u8; unsafe { CMSG_SPACE(mem::size_of::<ucred>() as u32) as usize }];
        let mut sender = unsafe { mem::zeroed::<sockaddr_nl>() };

        loop {
            let mut iov = iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: buf.len(),
            };
            let mut msg = unsafe { mem::zeroed::<msghdr>() };
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = cred_msg.as_mut_ptr().cast();
            msg.msg_controllen = cred_msg.len() as _;
            msg.msg_name = (&raw mut sender).cast();
            msg.msg_namelen = mem::size_of_val(&sender) as u32;

            let buflen = unsafe { cvt_r(|| recvmsg(self.as_raw_fd(), &mut msg, 0))? };
            if buflen < 32 || buflen >= buf.len() as isize {
                log::debug!("ignoring message: recvmsg returned invalid message of {buflen} bytes");
                continue;
            }

            log::trace!(
                "got {buflen} byte message from pid {} (groups={})",
                sender.nl_pid,
                sender.nl_groups,
            );

            if sender.nl_groups == 0 {
                log::debug!("ignoring unicast message");
                continue;
            } else if sender.nl_groups == MonitorNetlinkGroup::Kernel as _ && sender.nl_pid != 0 {
                log::debug!(
                    "ignoring kernel message from non-kernel process {}",
                    sender.nl_pid
                );
                continue;
            }

            // Check that the sender is root.
            // Importantly, none of the `CMSG_*` stuff guarantees a properly aligned pointer.
            // libudev doesn't seem to care and dereferences them anyways; Rust has a debug
            // assertion to catch that.
            let cmsg = unsafe { CMSG_FIRSTHDR(&msg) };
            if cmsg.is_null() {
                log::debug!("ignoring message: no credentials received");
                continue;
            }
            let cmsg_type = unsafe { cmsg.read_unaligned().cmsg_type };
            if cmsg_type != SCM_CREDENTIALS {
                log::debug!(
                    "ignoring message: received {} instead of {} (SCM_CREDENTIALS)",
                    cmsg_type,
                    SCM_CREDENTIALS,
                );
                continue;
            }

            let cred = unsafe { CMSG_DATA(cmsg).cast::<ucred>().read_unaligned() };
            if cred.uid != 0 {
                log::debug!(
                    "ignoring message: sent by uid {} instead of 0 (root)",
                    cred.uid
                );
                continue;
            }

            // At least the first 32 bytes of `buf` are valid.
            if !buf.starts_with(UDEV_PROLOG) {
                log::debug!("ignoring message: doesn't start with magic 'libudev' string");
                continue;
            }

            let header: &udev_monitor_netlink_header = unsafe { &*buf.as_ptr().cast() };
            log::trace!("udev message header: {header:?}");
            if header.magic != UDEV_MONITOR_MAGIC {
                log::debug!(
                    "ignoring message: mismatched magic number {:#?}",
                    identity(header.magic),
                );
                continue;
            }

            if header.properties_off > buflen as c_uint - 32 {
                log::debug!("invalid `properties_off`: {header:?}");
                continue;
            }

            // The event properties are a sequence of 0-terminated KEY=value pairs.
            let properties =
                &buf[header.properties_off as usize..][..header.properties_len as usize];
            let mut subsystem_input = false;
            let mut action_add = false;
            let mut devname = None;
            for entry in properties.split(|elem| *elem == 0) {
                if entry.is_empty() {
                    continue;
                }
                let s = String::from_utf8_lossy(entry);
                log::trace!("- {s}");

                // We're interested in the `DEVNAME` property (path in `/dev`) of events with
                // `SUBSYSTEM=input` and `ACTION=add`.
                if entry == b"SUBSYSTEM=input" {
                    subsystem_input = true;
                }
                if entry == b"ACTION=add" {
                    action_add = true;
                }
                if let Some(path) = entry.strip_prefix(b"DEVNAME=") {
                    devname = Some(path);
                }
            }

            if subsystem_input && action_add {
                if let Some(path) = devname {
                    if path.starts_with(b"/dev/input/event") {
                        let path = PathBuf::from(OsStr::from_bytes(path));
                        log::debug!("match! got hotplug event for: {}", path.display());
                        return Ok(HotplugEvent { path });
                    }
                }
            }

            log::trace!("no match");
        }
    }
}
