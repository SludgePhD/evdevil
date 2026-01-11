//! Device enumeration.
//!
//! Applications can choose whether they are only interested in the currently plugged-in devices
//! (via [`enumerate`]), or whether they also want to receive any devices that will be hot-plugged
//! in later (via [`enumerate_hotplug`]).
//!
//! Device enumeration is always blocking, and cannot be made non-blocking or `async`.
//! For interactive applications, it is recommended to perform device enumeration in a dedicated
//! thread.

use std::{
    cmp,
    collections::HashMap,
    fs::{self, ReadDir},
    io,
    os::unix::fs::FileTypeExt as _,
    path::PathBuf,
    thread,
    time::Duration,
    vec,
};

use crate::{Evdev, hotplug::HotplugMonitor};

/// Enumerates all currently plugged-in [`Evdev`] devices.
///
/// Performing enumeration can block for a significant amount of time while opening the *evdev*
/// device files. In user-facing applications, it is recommended to perform enumeration in a
/// background thread.
///
/// # Examples
///
/// ```
/// use evdevil::enumerate;
///
/// for res in enumerate()? {
///     let (path, evdev) = res?;
///     println!("{}: {}", path.display(), evdev.name()?);
/// }
/// # Ok::<_, std::io::Error>(())
/// ```
pub fn enumerate() -> io::Result<Enumerate> {
    Ok(Enumerate {
        read_dir: fs::read_dir("/dev/input")?,
    })
}

/// Enumerates all currently plugged-in [`Evdev`] devices, and future hotplugged devices.
///
/// The returned iterator will first yield the devices currently present on the system (like
/// [`enumerate`]), and then blocks until new devices are plugged into the system (using
/// [`HotplugMonitor`]).
///
/// This allows an application to process a single stream of [`Evdev`]s to both open an already
/// plugged-in device on startup, but also to react to hot-plugged devices automatically, which is
/// typically the desired UX of applications.
///
/// If opening the [`HotplugMonitor`] fails, this will degrade gracefully and only yield the
/// currently plugged-in devices.
///
/// # Examples
///
/// ```no_run
/// use evdevil::enumerate_hotplug;
///
/// for res in enumerate_hotplug()? {
///     let (path, evdev) = res?;
///     println!("{}: {}", path.display(), evdev.name()?);
/// }
/// # Ok::<_, std::io::Error>(())
/// ```
pub fn enumerate_hotplug() -> io::Result<EnumerateHotplug> {
    EnumerateHotplug::new()
}

/// Iterator over evdev devices on the system.
///
/// Returned by [`enumerate`].
///
/// If a device is plugged into the system after [`enumerate`] has been called, it is unspecified
/// whether [`Enumerate`] will yield the new device.
#[derive(Debug)]
pub struct Enumerate {
    read_dir: ReadDir,
}

impl Iterator for Enumerate {
    type Item = io::Result<(PathBuf, Evdev)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.read_dir.next()? {
                Ok(ent) => ent,
                Err(e) => return Some(Err(e)),
            };

            // Valid evdev devices are named `eventN`. `/dev/input` also contains some other
            // devices like `/dev/input/mouseN` that we have to skip.
            if !entry.file_name().as_encoded_bytes().starts_with(b"event") {
                continue;
            }

            let path = entry.path();
            let mkerr = |ioerr: io::Error| -> io::Error {
                io::Error::new(
                    ioerr.kind(),
                    format!("failed to access '{}': {}", path.display(), ioerr),
                )
            };

            let ty = match entry.file_type() {
                Ok(ty) => ty,
                Err(e) => return Some(Err(mkerr(e))),
            };
            if !ty.is_char_device() {
                continue;
            }

            match Evdev::open_unchecked(&path) {
                Ok(dev) => return Some(Ok((path, dev))),
                // If a device is unplugged in the middle of enumeration (before it can be opened),
                // skip it, since yielding this error to the application is pretty useless.
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Enumerates all current devices, and future hotplugged devices.
///
/// Returned by [`enumerate_hotplug`].
#[derive(Debug)]
pub struct EnumerateHotplug {
    to_yield: vec::IntoIter<io::Result<(PathBuf, Evdev)>>,

    monitor: Option<HotplugMonitor>,
    delay_ms: u32,
}

const INITIAL_DELAY: u32 = 250;
const MAX_DELAY: u32 = 8000;

impl EnumerateHotplug {
    fn new() -> io::Result<Self> {
        // The hotplug monitor has to be opened first, to ensure that devices plugged in during
        // enumeration are not lost.
        let monitor = match HotplugMonitor::new() {
            Ok(m) => Some(m),
            Err(e) => {
                log::warn!("couldn't open hotplug monitor: {e}; device hotplug will not work");
                None
            }
        };

        // If a device is plugged in during enumeration, it may be yielded twice: once from the
        // `readdir`-based enumeration, and once from the hotplug event.
        // To prevent that, we collect all `readdir` devices into a collection, and then drain all
        // pending hotplug events, ignoring those that belong to devices that we've already
        // collected (and that haven't been unplugged and replugged).
        // The resulting collection of devices is then yielded to the application, followed by any
        // hotplug events that arrive after the `readdir` enumeration is complete.

        let mut results = Vec::new();
        let mut path_map = HashMap::new();
        for res in enumerate()? {
            match res {
                Ok((path, evdev)) => {
                    let index = results.len();
                    results.push(Ok((path.clone(), evdev)));
                    path_map.insert(path, index);
                }
                Err(e) => results.push(Err(e)),
            }
        }
        if cfg!(test) {
            thread::sleep(Duration::from_millis(500));
        }

        if let Some(mon) = &monitor {
            mon.set_nonblocking(true)?;

            for res in mon {
                let Ok(event) = res else {
                    break;
                };

                match path_map.get(event.path()) {
                    Some(&i) => {
                        match &results[i] {
                            Ok((path, evdev)) if evdev.driver_version().is_ok() => {
                                // This device is still plugged in. Ignore this `HotplugEvent`.
                                log::debug!("device at `{}` still present", path.display());
                                continue;
                            }
                            _ => {
                                // Try opening the device.
                                log::debug!(
                                    "device at `{}` unplugged or errored; reopening",
                                    event.path().display()
                                );
                                results[i] = event.open().map(|evdev| (event.into_path(), evdev));
                            }
                        }
                    }
                    None => {
                        // This is a device path we haven't seen before, so it's a newly plugged-in
                        // device.
                        log::debug!(
                            "found new device during enumeration: {}",
                            event.path().display()
                        );
                        let index = results.len();
                        let res = event
                            .open()
                            .map(|evdev| (event.path().to_path_buf(), evdev));
                        results.push(res);
                        path_map.insert(event.into_path(), index);
                    }
                }
            }

            mon.set_nonblocking(false)?;
        }

        Ok(Self {
            to_yield: results.into_iter(),
            monitor,
            delay_ms: INITIAL_DELAY,
        })
    }
}

impl Iterator for EnumerateHotplug {
    type Item = io::Result<(PathBuf, Evdev)>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(res) = self.to_yield.next() {
            return Some(res);
        }

        let mon = match &mut self.monitor {
            Some(mon) => mon,
            None => loop {
                // The connection to the hotplug monitor was broken. Back off and try to reconnect.
                thread::sleep(Duration::from_millis(self.delay_ms.into()));
                self.delay_ms = cmp::min(self.delay_ms * 2, MAX_DELAY);
                match HotplugMonitor::new() {
                    Ok(mon) => {
                        #[cfg(test)]
                        mon.set_nonblocking(true).unwrap();

                        break self.monitor.insert(mon);
                    }
                    Err(e) => log::warn!("hotplug monitor reconnect failed: {e}"),
                }
            },
        };

        match mon.iter().next()? {
            Ok(event) => {
                let res = event.open().map(|dev| (event.into_path(), dev));
                Some(res)
            }
            Err(e) => {
                // If there's an error trying to receive a hotplug event, treat the socket
                // as broken and reconnect next time the iterator is advanced.
                self.monitor = None;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{event::Key, uinput::UinputDevice};

    use super::*;

    #[test]
    fn hotplug_reconnect() {
        let mut e = EnumerateHotplug {
            to_yield: Vec::new().into_iter(),
            monitor: None,
            delay_ms: 25,
        };

        e.next(); // may be `None` or `Some` if an event arrived
        assert!(e.monitor.is_some());
    }

    #[test]
    fn hotplug_enumerate() {
        if !fs::exists("/dev/uinput").unwrap() {
            eprintln!("`/dev/uinput` doesn't exist, probably running under QEMU");
            return;
        }

        env_logger::builder()
            .filter_module(env!("CARGO_PKG_NAME"), log::LevelFilter::Debug)
            .init();

        let h = thread::spawn(|| -> io::Result<()> {
            thread::sleep(Duration::from_millis(5));
            let _uinput = UinputDevice::builder()?
                .with_keys([Key::BTN_LEFT])?
                .build(&format!("@@@hotplugtest-early"))?;
            thread::sleep(Duration::from_millis(1000));
            Ok(())
        });

        let iter = enumerate_hotplug().unwrap();
        drop(iter);

        h.join().unwrap().unwrap();
    }
}
