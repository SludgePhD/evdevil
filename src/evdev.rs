use std::{
    error::Error,
    ffi::{c_char, c_int, c_uint, c_void},
    fmt,
    fs::File,
    io::{self, Read as _, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsFd, AsRawFd, IntoRawFd},
        unix::prelude::{BorrowedFd, RawFd},
    },
    path::{Path, PathBuf},
    slice,
    time::Instant,
};

use libc::clockid_t;
use uoctl::Ioctl;

use crate::{
    AbsInfo, InputProp, KeyRepeat, KeymapEntry, Version,
    bits::{BitSet, BitValue, Word},
    event::{
        Abs, EventType, ForceFeedbackEvent, InputEvent, Key, Led, LedEvent, Misc, Rel, Sound,
        Switch,
    },
    ff,
    input_id::InputId,
    keymap_entry::Scancode,
    raw::input::{
        EVIOCGABS, EVIOCGBIT, EVIOCGEFFECTS, EVIOCGID, EVIOCGKEY, EVIOCGKEYCODE_V2, EVIOCGLED,
        EVIOCGMASK, EVIOCGNAME, EVIOCGPHYS, EVIOCGPROP, EVIOCGRAB, EVIOCGREP, EVIOCGSND, EVIOCGSW,
        EVIOCGUNIQ, EVIOCGVERSION, EVIOCREVOKE, EVIOCRMFF, EVIOCSABS, EVIOCSCLOCKID, EVIOCSFF,
        EVIOCSKEYCODE_V2, EVIOCSMASK, EVIOCSREP, INPUT_KEYMAP_BY_INDEX, input_mask,
    },
    reader::EventReader,
    util::{block_until_readable, is_readable, set_nonblocking},
};

/// A handle to an *event device*.
///
/// A device can be opened via [`Evdev::open`] or by iterating over all evdev devices using
/// [`enumerate`] or [`enumerate_hotplug`].
///
/// [`Evdev`]s support non-blocking I/O for reading and writing events (but not for any
/// functionality that uses ioctls), which can be enabled or disabled by calling
/// [`Evdev::set_nonblocking`].
///
/// Just like [`File`]s, [`TcpStream`]s, and other wrappers around file descriptors, [`Evdev`] can
/// be duplicated by calling [`Evdev::try_clone`]. The underlying handle will be shared between
/// all cloned instances.
///
/// Since multiple [`Evdev`]s can refer to the same file handle, none of the methods require a
/// mutable reference, again mirroring the API of [`TcpStream`].
///
/// [`TcpStream`]: std::net::TcpStream
/// [`enumerate`]: crate::enumerate()
/// [`enumerate_hotplug`]: crate::enumerate_hotplug
#[derive(Debug)]
pub struct Evdev {
    pub(crate) file: File,
    path: PathBuf,
}

impl AsFd for Evdev {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        // Safety: we own the fd, so this lifetime constrains it properly
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

impl AsRawFd for Evdev {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl IntoRawFd for Evdev {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        self.file.into_raw_fd()
    }
}

impl Evdev {
    /// Opens a filesystem path referring to an `evdev` node.
    ///
    /// The path must belong to an `evdev` device like `/dev/input/event*`, not to a legacy
    /// *"joydev"* device (`/dev/input/js`) and not to a legacy *"mousedev"* (`/dev/input/mouse` or
    /// `/dev/input/mice`).
    ///
    /// # Permissions
    ///
    /// This method will attempt to open `path` with read-write permissions (allowing methods based
    /// on [`Evdev::write`] to work), fall back to read-only permissions if the current user
    /// does not have read and write permissions, and finally fall back to write-only permissions.
    ///
    /// If all of these attempts fail with a [`io::ErrorKind::PermissionDenied`] error, this method
    /// will return that error to the caller.
    ///
    /// # Errors
    ///
    /// This method will return an error if `path` doesn't refer to a path matching
    /// `/dev/input/event*` (after resolving symlinks).
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref();
        Self::open_impl(path)
    }

    fn open_impl(path: &Path) -> io::Result<Self> {
        let path = path.canonicalize()?;
        if !path
            .as_os_str()
            .as_encoded_bytes()
            .starts_with(b"/dev/input/event")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "evdev device path '{}' must match '/dev/input/event*'",
                    path.display()
                ),
            ));
        }

        Self::open_unchecked(path)
    }

    /// Opens `path` without checking that it is one of the `/dev/input/event*` paths.
    pub(crate) fn open_unchecked(path: PathBuf) -> io::Result<Self> {
        let now = Instant::now();

        let file = match Self::try_open(&path) {
            Ok(file) => file,
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("failed to open '{}': {e}", path.display()),
                ));
            }
        };
        let this = Self { file, path };
        let version = this.driver_version()?;
        log::debug!(
            "opened '{}' in {:?}; driver version {version}",
            this.path().display(),
            now.elapsed(),
        );
        Ok(this)
    }

    fn try_open(path: &Path) -> io::Result<File> {
        match File::options().read(true).write(true).open(path) {
            Ok(file) => return Ok(file),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                log::warn!(
                    "no permission to open '{}' in read-write mode, retrying in read-only",
                    path.display()
                );
            }
            Err(e) => return Err(e),
        }

        match File::options().read(true).open(path) {
            Ok(file) => return Ok(file),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                log::warn!(
                    "no permission to open '{}' in read-only mode, retrying in write-only",
                    path.display()
                );
            }
            Err(e) => return Err(e),
        }

        File::options().write(true).open(path)
    }

    /// Returns the (canonicalized) file system path this [`Evdev`] has been created from.
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Moves this handle into or out of non-blocking mode.
    ///
    /// Returns whether the [`Evdev`] was previously in non-blocking mode.
    ///
    /// [`Evdev`]s start out in *blocking* mode, in which every attempt to read from the device
    /// (either via [`Evdev::raw_events`] or via [`EventReader`]) will *block* until the next event
    /// is available.
    ///
    /// If the [`Evdev`] is put in non-blocking mode, attempts to read from it will no longer block,
    /// but instead fail with [`io::ErrorKind::WouldBlock`].
    ///
    /// This mechanism, alongside the [`AsRawFd`] impl, can be used to integrate [`Evdev`] into an
    /// `async` runtime. It can also be used by applications that want to occasionally retrieve all
    /// the device state, but don't want to block until new events are available (eg. games).
    ///
    /// **Note**: Non-blocking mode only works for reading and writing *events*. It does not work
    /// for any of the other device functionality, like force-feedback effect upload, which will
    /// always block.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<bool> {
        set_nonblocking(self.as_raw_fd(), nonblocking)
    }

    /// Creates a new [`Evdev`] instance that refers to the same underlying file handle.
    ///
    /// All state of the [`Evdev`] will be shared between the instances.
    ///
    /// **Note**: Care must be taken when using this method.
    /// Functionality in this crate (like [`EventReader`]) may assume that no other file handle is
    /// used to modify device state or read events from it.
    #[doc(alias = "dup")]
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            path: self.path.clone(),
        })
    }

    /// Executes `ioctl` and adds context to the error.
    pub(crate) unsafe fn ioctl<T>(
        &self,
        name: &'static str,
        ioctl: Ioctl<T>,
        arg: T,
    ) -> io::Result<c_int> {
        match unsafe { ioctl.ioctl(self, arg) } {
            Ok(ok) => Ok(ok),
            Err(e) => {
                #[derive(Debug)]
                struct WrappedError {
                    cause: io::Error,
                    msg: String,
                }

                impl fmt::Display for WrappedError {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(&self.msg)
                    }
                }
                impl Error for WrappedError {
                    fn source(&self) -> Option<&(dyn Error + 'static)> {
                        Some(&self.cause)
                    }
                }

                let msg = format!(
                    "ioctl {name} failed for device {} ({:?})",
                    self.path().display(),
                    e.kind()
                );
                Err(io::Error::new(e.kind(), WrappedError { cause: e, msg }))
            }
        }
    }

    unsafe fn fetch_string(
        &self,
        ioctl_name: &'static str,
        ioctl: fn(usize) -> Ioctl<*mut c_char>,
    ) -> io::Result<String> {
        // "fetch string" ioctls will return the number of bytes they've copied into our buffer.
        // This will be at most the length of the buffer. If that happens, some bytes might be lost,
        // so we retry the call after doubling the buffer size.

        const INITIAL_LEN: usize = 64;
        let mut buf = vec![0_u8; INITIAL_LEN];
        let len = loop {
            let len = unsafe {
                self.ioctl(
                    ioctl_name,
                    ioctl(buf.len()),
                    buf.as_mut_ptr() as *mut c_char,
                )?
            };
            if len as usize == buf.len() {
                // Not enough space; double the buffer size and retry.
                buf.resize(buf.len() * 2, 0);
            } else {
                break len;
            }
        };

        // `len` includes the trailing 0 byte
        buf.truncate(len.saturating_sub(1) as usize);

        let string =
            String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(string)
    }

    unsafe fn fetch_bits<V: BitValue>(
        &self,
        ioctl_name: &'static str,
        ioctl: fn(usize) -> Ioctl<*mut c_void>,
    ) -> io::Result<BitSet<V>> {
        let mut set = BitSet::<V>::new();
        let words = set.words_mut();
        unsafe {
            self.ioctl(
                ioctl_name,
                ioctl(words.len() * size_of::<Word>()),
                words.as_mut_ptr().cast(),
            )?;
        };
        Ok(set)
    }

    /// Returns the evdev subsystem version.
    #[doc(alias = "EVIOCGVERSION")]
    pub fn driver_version(&self) -> io::Result<Version> {
        unsafe {
            let mut version = 0;
            self.ioctl("EVIOCGVERSION", EVIOCGVERSION, &mut version)?;
            Ok(Version(version))
        }
    }

    /// Fetches device hardware information as an [`InputId`].
    #[doc(alias = "EVIOCGID")]
    pub fn input_id(&self) -> io::Result<InputId> {
        let mut out = MaybeUninit::uninit();
        unsafe {
            self.ioctl("EVIOCGID", EVIOCGID, out.as_mut_ptr())?;
            Ok(InputId(out.assume_init()))
        }
    }

    /// Fetches the device name.
    #[doc(alias = "EVIOCGNAME")]
    pub fn name(&self) -> io::Result<String> {
        unsafe { self.fetch_string("EVIOCGNAME", EVIOCGNAME) }
    }

    /// Fetches a string describing the physical location of the device.
    ///
    /// Possible location strings might look like:
    /// - `usb-0000:02:00.0-5/input1`
    /// - `PNP0C0C/button/input0`
    /// - `ALSA`
    ///
    /// Returns [`None`] when the device does not specify a physical location, typically because it
    /// is a virtual device with no associated physical location.
    /// However, note that virtual uinput devices *are allowed* to set this value to any string,
    /// which can be done via [`Builder::with_phys`][crate::uinput::Builder::with_phys].
    #[doc(alias = "EVIOCGPHYS")]
    pub fn phys(&self) -> io::Result<Option<String>> {
        unsafe {
            match self.fetch_string("EVIOCGPHYS", EVIOCGPHYS) {
                Ok(loc) => Ok(Some(loc)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    /// Fetches the unique identifier of this device.
    ///
    /// For USB devices, this is typically the device serial number (`iSerial`), which is often just
    /// the empty string (rather than [`None`]).
    #[doc(alias = "EVIOCGUNIQ")]
    pub fn unique_id(&self) -> io::Result<Option<String>> {
        unsafe {
            match self.fetch_string("EVIOCGUNIQ", EVIOCGUNIQ) {
                Ok(loc) => Ok(Some(loc)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    /// Fetches the set of [`InputProp`]s advertised by the device.
    #[doc(alias = "EVIOCGPROP")]
    pub fn props(&self) -> io::Result<BitSet<InputProp>> {
        unsafe { self.fetch_bits("EVIOCGPROP", EVIOCGPROP) }
    }

    /// Returns the set of supported [`EventType`]s.
    #[doc(alias = "EVIOCGBIT")]
    pub fn supported_events(&self) -> io::Result<BitSet<EventType>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(0, len)) }
    }

    /// Returns the set of supported [`Key`]s.
    pub fn supported_keys(&self) -> io::Result<BitSet<Key>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::KEY.0 as u8, len)) }
    }

    /// Returns the set of supported [`Switch`]es.
    pub fn supported_switches(&self) -> io::Result<BitSet<Switch>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::SW.0 as u8, len)) }
    }

    /// Returns the set of supported [`Led`]s.
    pub fn supported_leds(&self) -> io::Result<BitSet<Led>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::LED.0 as u8, len)) }
    }

    /// Returns the set of supported [`Sound`]s.
    pub fn supported_sounds(&self) -> io::Result<BitSet<Sound>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::SND.0 as u8, len)) }
    }

    /// Returns the set of supported [`Misc`] event codes.
    pub fn supported_misc(&self) -> io::Result<BitSet<Misc>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::MSC.0 as u8, len)) }
    }

    /// Returns the set of supported [`Rel`] axes.
    pub fn supported_rel_axes(&self) -> io::Result<BitSet<Rel>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::REL.0 as u8, len)) }
    }

    /// Returns the set of supported [`Abs`] axes.
    pub fn supported_abs_axes(&self) -> io::Result<BitSet<Abs>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::ABS.0 as u8, len)) }
    }

    /// Returns the set of supported force-feedback [`Feature`][ff::Feature]s.
    pub fn supported_ff_features(&self) -> io::Result<BitSet<ff::Feature>> {
        unsafe { self.fetch_bits("EVIOCGBIT", |len| EVIOCGBIT(EventType::FF.0 as u8, len)) }
    }

    /// Returns the number of force-feedback effects the device can store at the same time.
    #[doc(alias = "EVIOCGEFFECTS")]
    pub fn supported_ff_effects(&self) -> io::Result<u32> {
        unsafe {
            let mut out = 0;
            self.ioctl("EVIOCGEFFECTS", EVIOCGEFFECTS, &mut out)?;
            Ok(out.try_into().unwrap())
        }
    }

    /// Returns information about absolute axis `abs`.
    ///
    /// The supported absolute axes can be queried by calling [`Evdev::supported_abs_axes`].
    ///
    /// Calling this with an [`Abs`] axis that isn't supported by the device will either return an
    /// error or return a meaningless [`AbsInfo`] object with arbitrary values.
    ///
    /// Note that many devices don't send the correct values by default, and that userspace
    /// applications can generally override these values via [`Evdev::set_abs_info`].
    #[doc(alias = "EVIOCGABS")]
    pub fn abs_info(&self, abs: Abs) -> io::Result<AbsInfo> {
        if abs.0 > Abs::MAX.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("absolute axis {:?} exceeds maximum axis value", abs),
            ));
        }

        unsafe {
            let mut out = MaybeUninit::uninit();
            self.ioctl("EVIOCGABS", EVIOCGABS(abs.0 as u8), out.as_mut_ptr())?;
            Ok(AbsInfo(out.assume_init()))
        }
    }

    /// Sets the [`AbsInfo`] data associated with absolute axis `abs`.
    ///
    /// The supported absolute axes can be queried by calling [`Evdev::supported_abs_axes`].
    ///
    /// This method should generally not be used by applications, as it modifies globally visible
    /// device properties and can lead to the device not working correctly.
    #[doc(alias = "EVIOCSABS")]
    pub fn set_abs_info(&self, abs: Abs, info: AbsInfo) -> io::Result<()> {
        if abs.0 > Abs::MAX.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("absolute axis {:?} exceeds maximum axis value", abs),
            ));
        }

        unsafe {
            self.ioctl("EVIOCSABS", EVIOCSABS(abs.raw() as u8), &info.0)?;
        }
        Ok(())
    }

    /// Grabs this input device, making its events unavailable to other programs.
    ///
    /// This can be undone by calling [`Evdev::ungrab`]. The kernel will automatically *ungrab* a
    /// grabbed device when the program closes its file descriptor.
    ///
    /// # Errors
    ///
    /// This will return an error of type [`io::ErrorKind::ResourceBusy`] if the device is already
    /// grabbed by an application (including *this* application; in other words, calling `grab()`
    /// twice in a row will error).
    #[doc(alias = "EVIOCGRAB")]
    pub fn grab(&self) -> io::Result<()> {
        unsafe {
            self.ioctl("EVIOCGRAB", EVIOCGRAB, 1)?;
            Ok(())
        }
    }

    /// Ungrabs this input device, making its events available to other programs again.
    ///
    /// Undoes the effect of [`Evdev::grab`].
    ///
    /// # Errors
    ///
    /// This will return an error of type [`io::ErrorKind::InvalidInput`] is the device is **not**
    /// currently grabbed.
    #[doc(alias = "EVIOCGRAB")]
    pub fn ungrab(&self) -> io::Result<()> {
        unsafe {
            self.ioctl("EVIOCGRAB", EVIOCGRAB, 0)?;
            Ok(())
        }
    }

    /// Revokes device access from this [`Evdev`] handle.
    ///
    /// This prevents this handle from receiving any more input events, and makes writes and ioctls
    /// (including later calls to this one) fail with `ENODEV`.
    #[doc(alias = "EVIOCREVOKE")]
    pub fn revoke(&self) -> io::Result<()> {
        unsafe {
            self.ioctl("EVIOCREVOKE", EVIOCREVOKE, 0)?;
            Ok(())
        }
    }

    /// Queries the current autorepeat settings.
    ///
    /// If the device doesn't support key repeat, this will return `Ok(None)`.
    /// Whether key repeat is supported can also be determined by checking whether
    /// [`EventType::REP`] is advertised by [`Evdev::supported_events`].
    #[doc(alias = "EVIOCGREP")]
    pub fn key_repeat(&self) -> io::Result<Option<KeyRepeat>> {
        unsafe {
            let mut rep = [0; 2];
            match self.ioctl("EVIOCGREP", EVIOCGREP, &mut rep) {
                Ok(_) => Ok(Some(KeyRepeat {
                    delay: rep[0] as u32,
                    period: rep[1] as u32,
                })),
                Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    /// Sets the device's autorepeat settings.
    ///
    /// Also see [`Evdev::key_repeat`].
    ///
    /// Note that this is a persistent global property that will affect all applications using this
    /// device.
    /// This function should only be called if the user explicitly requested such a change.
    ///
    /// # Errors
    ///
    /// Not all devices support key repeat. If this method is called on a device that doesn't
    /// support it, an [`io::ErrorKind::Unsupported`] error may be returned.
    /// To determine whether key repeat is supported, check whether
    /// [`EventType::REP`] is advertised by [`Evdev::supported_events`].
    #[doc(alias = "EVIOCSREP")]
    pub fn set_key_repeat(&self, rep: KeyRepeat) -> io::Result<()> {
        unsafe {
            let rep = [rep.delay() as c_uint, rep.period() as c_uint];
            self.ioctl("EVIOCSREP", EVIOCSREP, &rep)?;
            Ok(())
        }
    }

    /// Queries a keymap entry by its associated [`Scancode`].
    ///
    /// Scancodes can appear in the keymap multiple times. Typically, the first entry takes
    /// precedence.
    ///
    /// The keymap can also be queried by index. See [`Evdev::keymap_entry_by_index`] for how to do
    /// that.
    /// The keymap can *not* be queried by *key* code.
    ///
    /// Devices without [`Key`]s don't have any keymap entries, and not all drivers support this
    /// functionality.
    ///
    /// Devices that support keymaps and have internal scancodes will typically send a
    /// [`Misc::SCAN`] event immediately before a key press or release event.
    ///
    /// Return `Ok(None)` when `index` is out of range.
    #[doc(alias = "EVIOCGKEYCODE_V2")]
    pub fn keymap_entry(&self, code: Scancode) -> io::Result<Option<KeymapEntry>> {
        unsafe {
            let mut entry = KeymapEntry::zeroed();
            entry.0.len = code.len;
            entry.0.scancode = code.bytes;
            match self.ioctl("EVIOCGKEYCODE_V2", EVIOCGKEYCODE_V2, &mut entry.0) {
                Ok(_) => Ok(Some(entry)),
                Err(e) if e.kind() == io::ErrorKind::InvalidInput => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    /// Queries a keymap entry by its zero-based index.
    ///
    /// The keymap can also be queried by scancode. See [`Evdev::keymap_entry`].
    /// The keymap can *not* be queried by *key* code.
    ///
    /// Devices without [`Key`]s don't have any keymap entries, and not all drivers support this
    /// functionality.
    ///
    /// Return `Ok(None)` when `index` is out of range.
    /// This is the only way to determine the number of entries in the keymap.
    #[doc(alias = "EVIOCGKEYCODE_V2")]
    pub fn keymap_entry_by_index(&self, index: u16) -> io::Result<Option<KeymapEntry>> {
        unsafe {
            let mut entry = KeymapEntry::zeroed();
            entry.0.index = index;
            entry.0.flags = INPUT_KEYMAP_BY_INDEX;
            match self.ioctl("EVIOCGKEYCODE_V2", EVIOCGKEYCODE_V2, &mut entry.0) {
                Ok(_) => Ok(Some(entry)),
                Err(e) if e.kind() == io::ErrorKind::InvalidInput => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    /// Sets a keymap entry by scancode.
    ///
    /// This will remap the given [`Scancode`] to produce `keycode`.
    ///
    /// Use with caution! The keymap is a global device property and changes to it will affect
    /// *every* application using that device.
    #[doc(alias = "EVIOCSKEYCODE_V2")]
    pub fn set_keymap_entry(&self, scancode: Scancode, keycode: Key) -> io::Result<()> {
        unsafe {
            let mut entry = KeymapEntry::zeroed();
            entry.0.keycode = keycode.raw().into();
            entry.0.len = scancode.len;
            entry.0.scancode = scancode.bytes;
            self.ioctl("EVIOCSKEYCODE_V2", EVIOCSKEYCODE_V2, &entry.0)?;
            Ok(())
        }
    }

    /// Sets a keymap entry by index.
    ///
    /// To find the [`Scancode`] at this index, or the valid indices, use
    /// [`Evdev::keymap_entry_by_index`].
    ///
    /// Use with caution! The keymap is a global device property and changes to it will affect
    /// *every* application using that device.
    #[doc(alias = "EVIOCSKEYCODE_V2")]
    pub fn set_keymap_entry_by_index(&self, index: u16, keycode: Key) -> io::Result<()> {
        unsafe {
            let mut entry = KeymapEntry::zeroed();
            entry.0.flags = INPUT_KEYMAP_BY_INDEX;
            entry.0.keycode = keycode.raw().into();
            entry.0.index = index;
            self.ioctl("EVIOCSKEYCODE_V2", EVIOCSKEYCODE_V2, &entry.0)?;
            Ok(())
        }
    }

    /// Queries the full key state.
    #[doc(alias = "EVIOCGKEY")]
    pub fn key_state(&self) -> io::Result<BitSet<Key>> {
        unsafe { self.fetch_bits("EVIOCGKEY", EVIOCGKEY) }
    }

    /// Queries the state of all device LEDs.
    #[doc(alias = "EVIOCGLED")]
    pub fn led_state(&self) -> io::Result<BitSet<Led>> {
        unsafe { self.fetch_bits("EVIOCGLED", EVIOCGLED) }
    }

    /// Queries the state of all [`Sound`]s.
    #[doc(alias = "EVIOCGSND")]
    pub fn sound_state(&self) -> io::Result<BitSet<Sound>> {
        unsafe { self.fetch_bits("EVIOCGSND", EVIOCGSND) }
    }

    /// Queries the state of all [`Switch`]es.
    #[doc(alias = "EVIOCGSW")]
    pub fn switch_state(&self) -> io::Result<BitSet<Switch>> {
        unsafe { self.fetch_bits("EVIOCGSW", EVIOCGSW) }
    }

    /// Creates an [`EventReader`] wrapping this device.
    ///
    /// This is the recommended way of receiving input events.
    /// The [`EventReader`] will automatically resynchronize with the kernel's view of the device
    /// when an event is lost due to overflow.
    pub fn into_reader(self) -> io::Result<EventReader> {
        EventReader::new(self)
    }

    /// Returns whether this device has any pending raw events that can be read without blocking.
    ///
    /// If this returns `true`, calling [`Evdev::raw_events()`] and then calling
    /// [`RawEvents::next()`] is guaranteed to not block (but only for a single event).
    /// Similarly, the next call to [`Evdev::read_events`] will not block.
    ///
    /// **Note**: This does not work for [`Evdev`]s wrapped in an [`EventReader`], since
    /// [`EventReader`] might read and discard several events from the underlying device. For
    /// updating an [`EventReader`] without blocking, use [`EventReader::update`].
    pub fn is_readable(&self) -> io::Result<bool> {
        is_readable(self.as_raw_fd())
    }

    /// Blocks the calling thread until [`Evdev::is_readable`] would return `true`.
    ///
    /// This will block even if the device has been put in non-blocking mode via
    /// [`Evdev::set_nonblocking`].
    /// For checking whether events can be read from the device without blocking, use
    /// [`Evdev::is_readable`], which will *never* block.
    ///
    /// If the device is already readable when this method is called, it will return immediately.
    pub fn block_until_readable(&self) -> io::Result<()> {
        block_until_readable(self.as_raw_fd())
    }

    /// Returns an iterator over the raw `evdev` events.
    ///
    /// This will directly read individual events from the `evdev`, without any buffering,
    /// filtering, synchronization on lost events, or fetching of the kernel's view of the device
    /// state.
    ///
    /// It is recommended to use [`Evdev::into_reader`] instead.
    ///
    /// [`RawEvents`] can be used (correctly) if the user is only interested in events pertaining to
    /// relative axes ([`RelEvent`][crate::event::RelEvent]), since those have no state.
    ///
    /// - If the device is in blocking mode, [`RawEvents::next`] will block until an event is
    ///   available.
    /// - If the [`Evdev`] is in non-blocking mode, the iterator will return [`None`] when reading
    ///   fails with a [`io::ErrorKind::WouldBlock`] error (and later calls to
    ///   [`Iterator::next`] may return [`Some`] again).
    ///
    /// Reading events individually can be slow, since every event incurs a system call.
    /// You can use [`Evdev::read_events`] to read them in larger batches.
    ///
    /// **Note**: If this method is used while the device is wrapped in an [`EventReader`], the
    /// [`EventReader`] will miss events and go out of sync with the device state. Don't do that.
    #[inline]
    pub fn raw_events(&self) -> RawEvents<'_> {
        RawEvents { file: &self.file }
    }

    /// Reads incoming raw events into `buf`.
    ///
    /// This may read multiple events at once, which is more efficient than using
    /// [`Evdev::raw_events`] to read them one-by-one.
    ///
    /// - If the device is in blocking mode, this method will block until at least 1 event can be
    ///   read.
    /// - If the device is in non-blocking mode, this method will return an error of type
    ///   [`io::ErrorKind::WouldBlock`] when there are no events to read.
    ///
    /// **Note**: If this method is used while the device is wrapped in an [`EventReader`], the
    /// [`EventReader`] will miss events and go out of sync with the device state. Don't do that.
    pub fn read_events(&self, buf: &mut [InputEvent]) -> io::Result<usize> {
        read_raw(&self.file, buf)
    }

    /// Uploads a force-feedback effect to the device.
    ///
    /// This is always a blocking operation, even if the [`Evdev`] is in non-blocking mode.
    /// If the device is a `uinput` device, bugs in the userspace driver might cause this to block
    /// for a *very* long time (there appears to be a timeout at 20-30 seconds).
    ///
    /// Also see [`Evdev::supported_ff_effects`] for the number of supported effect slots, and
    /// [`Evdev::supported_ff_features`] for the supported force-feedback feature set.
    ///
    /// Uploaded effects will stay in device memory until removed via [`Evdev::erase_ff_effect`].
    #[doc(alias = "EVIOCSFF")]
    pub fn upload_ff_effect<'a>(
        &self,
        effect: impl Into<ff::Effect<'a>>,
    ) -> io::Result<ff::EffectId> {
        self.upload_ff_effect_impl(effect.into())
    }
    fn upload_ff_effect_impl(&self, mut effect: ff::Effect<'_>) -> io::Result<ff::EffectId> {
        log::trace!("uploading FF effect: {:?}", effect);
        let now = Instant::now();
        unsafe {
            self.ioctl("EVIOCSFF", EVIOCSFF, &mut effect.raw)?;
        }
        log::debug!("upload_ff_effect: ioctl took {:?}", now.elapsed());

        Ok(ff::EffectId(effect.raw.id))
    }

    /// Deletes a previously uploaded force-feedback effect.
    #[doc(alias = "EVIOCRMFF")]
    pub fn erase_ff_effect(&self, id: ff::EffectId) -> io::Result<()> {
        unsafe {
            self.ioctl("EVIOCRMFF", EVIOCRMFF, id.0 as c_int)?;
        }
        Ok(())
    }

    /// Sets the state of a device LED.
    ///
    /// To query the list of LEDs available on the device, use [`Evdev::supported_leds`].
    ///
    /// This is a convenience wrapper around [`Evdev::write`] that sends a [`LedEvent`]
    /// to the device.
    pub fn set_led(&self, led: Led, on: bool) -> io::Result<()> {
        self.write(&[LedEvent::new(led, on).into()])
    }

    /// Starts or stops a force-feedback effect (eg. [`ff::Rumble`]).
    ///
    /// Before an effect can be started with this method, it needs to be uploaded via
    /// [`Evdev::upload_ff_effect`].
    ///
    /// This is a convenience wrapper around [`Evdev::write`] that sends a [`ForceFeedbackEvent`]
    /// to the device.
    pub fn control_ff(&self, effect: ff::EffectId, active: bool) -> io::Result<()> {
        self.write(&[ForceFeedbackEvent::control_effect(effect, active).into()])
    }

    /// Sets the global gain for force-feedback effects.
    ///
    /// The `gain` value encodes the gain as a fraction of 65535 (100%).
    ///
    /// Requires that the device supports [`ff::Feature::GAIN`].
    ///
    /// This is a convenience wrapper around [`Evdev::write`] that sends a [`ForceFeedbackEvent`]
    /// to the device.
    pub fn set_ff_gain(&self, gain: u16) -> io::Result<()> {
        self.write(&[ForceFeedbackEvent::control_gain(gain).into()])
    }

    /// Controls the autocenter feature for force-feedback effects.
    ///
    /// The `autocenter` value encodes the autocenter power as a fraction of 65535 (100%).
    ///
    /// Requires that the device supports [`ff::Feature::AUTOCENTER`].
    ///
    /// This is a convenience wrapper around [`Evdev::write`] that sends a [`ForceFeedbackEvent`]
    /// to the device.
    pub fn set_ff_autocenter(&self, autocenter: u16) -> io::Result<()> {
        self.write(&[ForceFeedbackEvent::control_autocenter(autocenter).into()])
    }

    /// Writes events to the device.
    ///
    /// This can be used to change certain device states such as LEDs or sounds, or to play
    /// force-feedback effects.
    ///
    /// Prefer using one of the convenience wrappers around this method if possible:
    /// [`Evdev::control_ff`], [`Evdev::set_led`], etc.
    ///
    /// **Note**: As per usual for file descriptors, writing data to the device is only possible if
    /// it was opened with write permission.
    /// [`Evdev::open`] will *try* to open the device with read+write permissions, and fall back to
    /// read-only mode if the user does not have write permission for the evdev files.
    /// If that also fails, one last attempt to open in *write-only* mode is made.
    ///
    /// If the [`Evdev`] does not have write permission, this method will fail with a
    /// [`io::ErrorKind::PermissionDenied`] error.
    pub fn write(&self, events: &[InputEvent]) -> io::Result<()> {
        unsafe {
            let bytes = slice::from_raw_parts(
                events.as_ptr().cast(),
                events.len() * size_of::<InputEvent>(),
            );
            (&self.file).write_all(bytes)
        }
    }

    /// Sets the [`clockid_t`] to be used for event timestamps.
    ///
    /// `evdev` doesn't support *all* clocks. This method will fail with an
    /// [`io::ErrorKind::InvalidInput`] error when a `clockid` is passed that the kernel doesn't
    /// like.
    /// At least [`libc::CLOCK_REALTIME`] and [`libc::CLOCK_MONOTONIC`] seem to be supported.
    ///
    /// By default, [`libc::CLOCK_REALTIME`] is used, which is the same clock source used by
    /// [`SystemTime::now`][std::time::SystemTime::now].
    ///
    /// If this is called while there are any events in the kernel buffer, the buffer will be
    /// cleared and a [`Syn::DROPPED`] event will be enqueued.
    ///
    /// [`Syn::DROPPED`]: crate::event::Syn::DROPPED
    #[doc(alias = "EVIOCSCLOCKID")]
    pub fn set_clockid(&self, clockid: clockid_t) -> io::Result<()> {
        unsafe {
            self.ioctl("EVIOCSCLOCKID", EVIOCSCLOCKID, &clockid)?;
            Ok(())
        }
    }
}

/// Event masks can be optionally configured to hide event types if a consumer isn't interested in
/// them.
///
/// This can help avoid wasted work and unnecessary process wakeups, which can save battery.
///
/// **Note**: Fetching the device state directly (via [`Evdev::key_state`] et al) will still report
/// the *true* device state, without any bits masked out. The event masks only control which
/// *events* are forwarded to the program.
///
/// **Note 2**: [`EventReader`] is not aware of event masks and may insert some synthetic events
/// that have been disabled with the event masks. Your application should configure the masks as
/// desired *and* actively ignore any events that it sees if it isn't interested in them.
///
/// # Platform Support
///
/// FreeBSD does not support these APIs, so they will return an error when used.
/// Applications should degrade gracefully when that happens, since the consequence of not filtering
/// events is merely a decrease in performance.
impl Evdev {
    fn fetch_mask<V: BitValue>(&self, ty: EventType) -> io::Result<BitSet<V>> {
        let mut set = BitSet::<V>::new();
        let words = set.words_mut();
        unsafe {
            let mut mask = input_mask {
                type_: ty.0.into(),
                codes_size: (words.len() * size_of::<Word>()) as u32,
                codes_ptr: words.as_mut_ptr().expose_provenance() as u64,
            };
            self.ioctl("EVIOCGMASK", EVIOCGMASK, &mut mask)?;
        }
        Ok(set)
    }

    fn set_mask<V: BitValue>(&self, ty: EventType, mask: &BitSet<V>) -> io::Result<()> {
        let words = mask.words();
        unsafe {
            let mask = input_mask {
                type_: ty.0.into(),
                codes_size: (words.len() * size_of::<Word>()) as u32,
                codes_ptr: words.as_ptr().expose_provenance() as u64,
            };
            self.ioctl("EVIOCSMASK", EVIOCSMASK, &mask)?;
        }
        Ok(())
    }

    /// Fetches the current event mask.
    #[doc(alias = "EVIOCGMASK")]
    pub fn event_mask(&self) -> io::Result<BitSet<EventType>> {
        self.fetch_mask(EventType::from_raw(0))
    }

    /// Sets the event mask.
    ///
    /// Only [`EventType`]s included in `mask` will be forwarded to this [`Evdev`] handle.
    ///
    /// **Note**: [`EventType::SYN`] cannot be the only enabled event type. At least one "real"
    /// (non-SYN) event has to be enabled, or **no** events will be forwarded to the program.
    #[doc(alias = "EVIOCSMASK")]
    pub fn set_event_mask(&self, mask: &BitSet<EventType>) -> io::Result<()> {
        self.set_mask(EventType::from_raw(0), mask)
    }

    /// Fetches the current key event mask.
    pub fn key_mask(&self) -> io::Result<BitSet<Key>> {
        self.fetch_mask(EventType::KEY)
    }

    /// Sets the key event mask.
    ///
    /// This [`Evdev`] handle will only receive [`KeyEvent`]s whose [`Key`] is contained in `mask`.
    ///
    /// [`KeyEvent`]: crate::event::KeyEvent
    pub fn set_key_mask(&self, mask: &BitSet<Key>) -> io::Result<()> {
        self.set_mask(EventType::KEY, mask)
    }

    /// Fetches the current relative axis event mask.
    pub fn rel_mask(&self) -> io::Result<BitSet<Rel>> {
        self.fetch_mask(EventType::REL)
    }

    /// Sets the relative axis event mask.
    pub fn set_rel_mask(&self, mask: &BitSet<Rel>) -> io::Result<()> {
        self.set_mask(EventType::REL, mask)
    }

    /// Fetches the current absolute axis event mask.
    pub fn abs_mask(&self) -> io::Result<BitSet<Abs>> {
        self.fetch_mask(EventType::ABS)
    }

    /// Sets the absolute axis event mask.
    pub fn set_abs_mask(&self, mask: &BitSet<Abs>) -> io::Result<()> {
        self.set_mask(EventType::ABS, mask)
    }

    /// Fetches the current switch event mask.
    pub fn switch_mask(&self) -> io::Result<BitSet<Switch>> {
        self.fetch_mask(EventType::SW)
    }

    /// Sets the switch event mask.
    pub fn set_switch_mask(&self, mask: &BitSet<Switch>) -> io::Result<()> {
        self.set_mask(EventType::SW, mask)
    }
}

/// Reads raw [`InputEvent`]s from an [`Evdev`].
///
/// Returned by [`Evdev::raw_events`].
///
/// This holds no state and performs no batching, so it can be created at will via
/// [`Evdev::raw_events`].
/// However, reading events this way is rather slow, so [`Evdev::read_events`] may be preferable
/// if performance is important.
///
/// [`UinputDevice`]: crate::uinput::UinputDevice
/// [`UinputDevice::events`]: crate::uinput::UinputDevice::events
#[derive(Debug)]
pub struct RawEvents<'a> {
    pub(crate) file: &'a File,
}

impl Iterator for RawEvents<'_> {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut dest = InputEvent::zeroed();
        match read_raw(&self.file, slice::from_mut(&mut dest)) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => Some(Err(e)),
            Ok(0) => None,
            Ok(1) => Some(Ok(dest)),
            Ok(n) => unreachable!("read {n} events, but can only hold 1"),
        }
    }
}

fn read_raw(mut file: &File, dest: &mut [InputEvent]) -> io::Result<usize> {
    let bptr = dest.as_mut_ptr().cast::<u8>();
    let byte_buf = unsafe { slice::from_raw_parts_mut(bptr, size_of::<InputEvent>() * dest.len()) };
    let bytes = file.read(byte_buf)?;
    debug_assert_eq!(bytes % size_of::<InputEvent>(), 0);
    Ok(bytes / size_of::<InputEvent>())
}
