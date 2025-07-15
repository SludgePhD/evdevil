use std::{
    ffi::c_int,
    io,
    os::fd::{AsRawFd, RawFd},
};

/// Uses `poll(2)` to determine whether reading from `fd` is possible without blocking.
pub fn is_readable(fd: RawFd) -> io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ret = unsafe { libc::poll(&mut poll, 1, 0) };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(poll.revents & libc::POLLIN != 0)
}

pub fn block_until_readable(fd: RawFd) -> io::Result<()> {
    loop {
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut poll, 1, -1) };
        if ret == -1 {
            return Err(io::Error::last_os_error());
        }

        if poll.revents & libc::POLLIN != 0 {
            // Is now readable.
            return Ok(());
        }
    }
}

pub fn set_nonblocking(fd: RawFd, nonblocking: bool) -> io::Result<bool> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }

    let was_nonblocking = flags & libc::O_NONBLOCK != 0;
    let new_flags = if nonblocking {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };

    if new_flags != flags {
        let ret = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, new_flags) };
        if ret == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(was_nonblocking)
}

pub fn errorkind2libc(kind: io::ErrorKind) -> Option<c_int> {
    use io::ErrorKind::*;

    // let us copy the existing translation straight from libstd
    macro_rules! do_a_flip {
        ( $( $libc:expr => $kind:pat, )* ) => {
            Some(match kind {
                $( $kind => $libc, )*

                // `Uncategorized`, `Other`, ...
                _ => return None
            })
        };
    }

    // from `decode_error_kind` in std/src/sys/pal/unix/mod.rs
    do_a_flip! {
        libc::E2BIG => ArgumentListTooLong,
        libc::EADDRINUSE => AddrInUse,
        libc::EADDRNOTAVAIL => AddrNotAvailable,
        libc::EBUSY => ResourceBusy,
        libc::ECONNABORTED => ConnectionAborted,
        libc::ECONNREFUSED => ConnectionRefused,
        libc::ECONNRESET => ConnectionReset,
        libc::EDEADLK => Deadlock,
        libc::EDQUOT => QuotaExceeded,
        libc::EEXIST => AlreadyExists,
        libc::EFBIG => FileTooLarge,
        libc::EHOSTUNREACH => HostUnreachable,
        libc::EINTR => Interrupted,
        libc::EINVAL => InvalidInput,
        libc::EISDIR => IsADirectory,
        //libc::ELOOP => FilesystemLoop,
        libc::ENOENT => NotFound,
        libc::ENOMEM => OutOfMemory,
        libc::ENOSPC => StorageFull,
        libc::ENOSYS => Unsupported,
        libc::EMLINK => TooManyLinks,
        //libc::ENAMETOOLONG => InvalidFilename,
        libc::ENETDOWN => NetworkDown,
        libc::ENETUNREACH => NetworkUnreachable,
        libc::ENOTCONN => NotConnected,
        libc::ENOTDIR => NotADirectory,
        libc::ENOTEMPTY => DirectoryNotEmpty,
        libc::EPIPE => BrokenPipe,
        libc::EROFS => ReadOnlyFilesystem,
        libc::ESPIPE => NotSeekable,
        libc::ESTALE => StaleNetworkFileHandle,
        libc::ETIMEDOUT => TimedOut,
        libc::ETXTBSY => ExecutableFileBusy,
        libc::EXDEV => CrossesDevices,
        //libc::EINPROGRESS => InProgress,
        // unstable ones are commented out

        // special cases

        // Map `PermissionDenied` to `EACCES`, not `EPERM`, because... I don't know either.
        // What's the difference?
        libc::EACCES => PermissionDenied,
        // Canonicalize to EWOULDBLOCK, not EAGAIN
        libc::EWOULDBLOCK => WouldBlock,
    }
}
