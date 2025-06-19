//! Userspace input devices.
//!
//! This module allows writing device drivers and virtual input devices in Rust.
//!
//! A [`UinputDevice`] can be created via [`UinputDevice::builder`] and will create a corresponding
//! evdev input device that other applications (or *this* application) can read events from.

use std::{
    error::Error,
    ffi::{CStr, CString, OsString, c_char, c_int},
    fmt,
    fs::File,
    io::{self, Read as _},
    mem::{self, MaybeUninit},
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd},
        unix::{ffi::OsStringExt, prelude::RawFd},
    },
    ptr, slice,
    time::Instant,
};

use linux_ioctl::Ioctl;

use crate::{
    AbsInfo, InputId, InputProp, KeyRepeat, Slot,
    batch::BatchWriter,
    drop::on_drop,
    event::{
        Abs, AbsEvent, EventType, InputEvent, Key, Led, Misc, Rel, Repeat, RepeatEvent, Sound,
        Switch, Syn, SynEvent, UinputCode, UinputEvent,
    },
    ff::{self, Effect, EffectId},
    raw::{
        input::ff_effect,
        uinput::{
            UI_ABS_SETUP, UI_BEGIN_FF_ERASE, UI_BEGIN_FF_UPLOAD, UI_DEV_CREATE, UI_DEV_SETUP,
            UI_END_FF_ERASE, UI_END_FF_UPLOAD, UI_GET_SYSNAME, UI_GET_VERSION, UI_SET_ABSBIT,
            UI_SET_EVBIT, UI_SET_FFBIT, UI_SET_KEYBIT, UI_SET_LEDBIT, UI_SET_MSCBIT, UI_SET_PHYS,
            UI_SET_PROPBIT, UI_SET_RELBIT, UI_SET_SNDBIT, UI_SET_SWBIT, UINPUT_MAX_NAME_SIZE,
            uinput_abs_setup, uinput_ff_erase, uinput_ff_upload, uinput_setup,
        },
    },
    util::{block_until_readable, errorkind2libc, is_readable, set_nonblocking},
};

/// Absolute axis setup information.
///
/// Used by [`Builder::with_abs_axes`].
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct AbsSetup(uinput_abs_setup);

impl fmt::Debug for AbsSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbsSetup")
            .field("abs", &self.abs())
            .field("abs_info", self.abs_info())
            .finish()
    }
}

impl AbsSetup {
    pub const fn new(abs: Abs, abs_info: AbsInfo) -> Self {
        AbsSetup(uinput_abs_setup {
            code: abs.raw(),
            absinfo: abs_info.0,
        })
    }

    pub const fn abs(&self) -> Abs {
        Abs::from_raw(self.0.code)
    }

    pub const fn abs_info(&self) -> &AbsInfo {
        // Safety: `AbsInfo` is a `#[repr(transparent)]` wrapper
        unsafe { mem::transmute(&self.0.absinfo) }
    }
}

/// A builder for creating a [`UinputDevice`].
///
/// Returned by [`UinputDevice::builder`].
pub struct Builder {
    file: File, // handle to `/dev/uinput`
    setup: uinput_setup,
}

impl fmt::Debug for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builder")
            .field("file", &self.file)
            .field("input_id", &InputId(self.setup.id))
            .field("ff_effects_max", &self.setup.ff_effects_max)
            .finish()
    }
}

impl Builder {
    fn new() -> io::Result<Self> {
        let file = File::options().read(true).write(true).open("/dev/uinput")?;
        unsafe {
            let mut version = 0;
            UI_GET_VERSION.ioctl(&file, &mut version)?;
            log::debug!("opened /dev/uinput; version={version:#x}");
        }
        Ok(Self {
            file,
            setup: unsafe { mem::zeroed() },
        })
    }

    /// Configures the device's hardware IDs.
    pub fn with_device_id(mut self, id: InputId) -> io::Result<Self> {
        self.setup.id = id.0;
        Ok(self)
    }

    /// Sets the physical path of the device.
    ///
    /// By default, the physical path of a `uinput` device is unset, and the corresponding
    /// [`Evdev::phys`] method will return [`None`].
    ///
    /// This method can be used to change that behavior and expose the proper hardware location to
    /// consumers.
    ///
    /// [`Evdev::phys`]: crate::Evdev::phys
    pub fn with_phys(self, path: &str) -> io::Result<Self> {
        self.with_phys_cstr(&CString::new(path).unwrap())
    }

    /// Sets the physical path of the device to a [`CStr`].
    ///
    /// It is typically easier to use [`Builder::with_phys`] instead.
    pub fn with_phys_cstr(self, path: &CStr) -> io::Result<Self> {
        unsafe {
            UI_SET_PHYS.ioctl(&self.file, path.as_ptr().cast())?;
        }

        Ok(self)
    }

    /// Sets the given [`InputProp`]s for the device.
    ///
    /// [`InputProp`]s can be used to advertise a specific type of device, like a drawing tablet.
    pub fn with_props(self, props: impl IntoIterator<Item = InputProp>) -> io::Result<Self> {
        for prop in props {
            unsafe {
                UI_SET_PROPBIT.ioctl(&self.file, prop.0.into())?;
            }
        }
        Ok(self)
    }

    /// Enables the given list of [`Key`]s to be reported by the device.
    pub fn with_keys(self, keys: impl IntoIterator<Item = Key>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_KEYBIT,
            EventType::KEY,
            keys.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of [`Rel`]ative axes to be reported by the device.
    pub fn with_rel_axes(self, rel: impl IntoIterator<Item = Rel>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_RELBIT,
            EventType::REL,
            rel.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of [`Misc`] events to be reported by the device.
    pub fn with_misc(self, misc: impl IntoIterator<Item = Misc>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_MSCBIT,
            EventType::MSC,
            misc.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of [`Led`]s.
    ///
    /// LEDs may be controlled by either the `uinput` or `evdev` side, by writing the appropriate
    /// event to the stream.
    pub fn with_leds(self, leds: impl IntoIterator<Item = Led>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_LEDBIT,
            EventType::LED,
            leds.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of [`Sound`]s.
    ///
    /// Sounds are typically played by an [`Evdev`][crate::Evdev] handle by writing the appropriate
    /// event to the stream.
    pub fn with_sounds(self, sounds: impl IntoIterator<Item = Sound>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_SNDBIT,
            EventType::SND,
            sounds.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of [`Switch`]es to be reported by the device.
    pub fn with_switches(self, switches: impl IntoIterator<Item = Switch>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_SWBIT,
            EventType::SW,
            switches.into_iter().map(|v| v.raw().into()),
        )?;
        Ok(self)
    }

    /// Enables the given list of absolute axes.
    ///
    /// The [`AbsInfo`] associated with an axis may be changed by an [`Evdev`][crate::Evdev] client
    /// via [`Evdev::set_abs_info`][crate::Evdev::set_abs_info].
    pub fn with_abs_axes(self, axes: impl IntoIterator<Item = AbsSetup>) -> io::Result<Self> {
        self.enable_event(EventType::ABS)?;
        for setup in axes {
            unsafe {
                UI_SET_ABSBIT.ioctl(&self.file, setup.0.code as c_int)?;
                UI_ABS_SETUP.ioctl(&self.file, &setup.0)?;
            }
        }
        Ok(self)
    }

    /// Sets the maximum number of force-feedback effects that can be played at once.
    ///
    /// If this is greater than 0, the device will advertise support for [`EventType::FF`] events.
    ///
    /// Note that you also have to enable the specific force-feedback features you intend to support
    /// by calling [`Builder::with_ff_features`].
    pub fn with_ff_effects_max(mut self, ff_max: u32) -> io::Result<Self> {
        self.setup.ff_effects_max = ff_max;
        Ok(self)
    }

    /// Advertises the given force-feedback capabilities.
    ///
    /// If you call this method, you also have to call [`Builder::with_ff_effects_max`] to configure
    /// the maximum number of force-feedback effects the device can accept, or the functionality
    /// won't work.
    pub fn with_ff_features(self, feat: impl IntoIterator<Item = ff::Feature>) -> io::Result<Self> {
        self.enable_codes(
            UI_SET_FFBIT,
            EventType::FF,
            feat.into_iter().map(|v| v.0.into()),
        )?;
        Ok(self)
    }

    /// Enables support for autorepeat.
    ///
    /// If this is called, [`RepeatEvent`]s may be written to the stream to change the autorepeat
    /// settings.
    /// This will also allow [`Evdev`][crate::Evdev] clients to query and modify the key repeat
    /// settings via [`Evdev::key_repeat`][crate::Evdev::key_repeat] and
    /// [`Evdev::set_key_repeat`][crate::Evdev::set_key_repeat].
    pub fn with_key_repeat(self) -> io::Result<Self> {
        // NOTE: cannot take the `KeyRepeat` as an argument because it has to be written to the stream
        self.enable_event(EventType::REP)?;
        Ok(self)
    }

    // Will return `EINVAL` when attempting to enable a code above the maximum for that type of code.
    fn enable_codes(
        &self,
        ioctl: Ioctl<c_int>,
        event: EventType,
        codes: impl IntoIterator<Item = usize>,
    ) -> io::Result<()> {
        // Note: these will all yield `EINVAL` with out-of-range indices
        self.enable_event(event)?;
        for code in codes {
            unsafe {
                ioctl.ioctl(&self.file, code as c_int)?;
            }
        }
        Ok(())
    }

    fn enable_event(&self, event: EventType) -> io::Result<()> {
        unsafe {
            UI_SET_EVBIT.ioctl(&self.file, event.0 as c_int)?;
        }
        Ok(())
    }

    /// Creates the `uinput` device.
    ///
    /// After this method returns successfully, the device will show up in `/dev/input` and emit
    /// hotplug events accordingly.
    ///
    /// **NOTE**: Because of how `udev` works, devices can show up with incorrect permission bits
    /// for a short time, before those permissions are set correctly by the system.
    /// This means that calling [`enumerate`][crate::enumerate] immediately after creating a
    /// `uinput` device (or immediately after plugging in a physical device) might fail to access
    /// the device.
    /// However, *hotplug* events should arrive only after the device has been given the correct
    /// permissions.
    ///
    /// # Parameters
    ///
    /// - `name`: The name of the device. Should be ASCII, and must not be longer than 79 bytes, or
    ///   this method will return an error.
    pub fn build(mut self, name: &str) -> io::Result<UinputDevice> {
        if name.len() >= UINPUT_MAX_NAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "uinput device name is too long",
            ));
        }

        unsafe {
            ptr::copy_nonoverlapping(
                name.as_ptr(),
                self.setup.name.as_mut_ptr().cast(),
                name.len(),
            );
        }

        unsafe {
            UI_DEV_SETUP.ioctl(&self.file, &self.setup)?;
            UI_DEV_CREATE.ioctl(&self.file)?;
        }
        Ok(UinputDevice { file: self.file })
    }
}

/// A virtual `uinput` device.
#[derive(Debug)]
pub struct UinputDevice {
    // NOTE: we deliberately don't call `UI_DEV_DESTROY` on drop, since there can be multiple
    // `UinputDevice` handles referring to the same device or file description due to `try_clone`.
    // Closing the last handle to the device will already make the kernel clean everything up
    // anyways, to using the ioctl seems unnecessary.
    file: File,
}

impl AsFd for UinputDevice {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        // Safety: we own the fd, so this lifetime constrains it properly
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

impl AsRawFd for UinputDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl IntoRawFd for UinputDevice {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        self.file.into_raw_fd()
    }
}

impl FromRawFd for UinputDevice {
    #[inline]
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self {
            file: unsafe { File::from_raw_fd(fd) },
        }
    }
}

impl UinputDevice {
    /// Returns a [`Builder`] for configuring a new input device.
    pub fn builder() -> io::Result<Builder> {
        Builder::new()
    }

    /// Moves this handle into or out of non-blocking mode.
    ///
    /// Returns whether the [`UinputDevice`] was previously in non-blocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<bool> {
        set_nonblocking(self.as_raw_fd(), nonblocking)
    }

    /// Creates a new [`UinputDevice`] instance that refers to the same underlying file handle.
    ///
    /// All properties, such as whether the handle is in non-blocking mode, will be shared between
    /// the instances.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
        })
    }

    /// Executes `ioctl` and adds context to the error.
    unsafe fn ioctl<T>(&self, name: &'static str, ioctl: Ioctl<T>, arg: T) -> io::Result<c_int> {
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

                log::trace!("ioctl {name} failed with error {e} ({:?})", e.kind());
                let msg = format!("ioctl {name} failed ({:?})", e.kind());
                Err(io::Error::new(e.kind(), WrappedError { cause: e, msg }))
            }
        }
    }

    unsafe fn fetch_string(
        &self,
        ioctl_name: &'static str,
        ioctl: fn(usize) -> Ioctl<*mut c_char>,
    ) -> io::Result<OsString> {
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

        Ok(OsString::from_vec(buf))
    }

    /// Retrieves the uinput device's directory name in the sysfs hierarchy.
    ///
    /// The full path to the directory is `/sys/devices/virtual/input/` followed by the name
    /// returned by this method.
    ///
    /// This functionality is generally non-portable and only works on Linux.
    pub fn sysname(&self) -> io::Result<OsString> {
        unsafe { self.fetch_string("UI_GET_SYSNAME", UI_GET_SYSNAME) }
    }

    /// Returns an iterator over events *received* by this [`UinputDevice`].
    ///
    /// If the device exposes any of the following functionality, it should read events that trigger
    /// it from this iterator and act accordingly:
    ///
    /// - LEDs (via [`Builder::with_leds`]).
    /// - Sounds (via [`Builder::with_sounds`]).
    /// - Force Feedback (via [`Builder::with_ff_features`] and [`Builder::with_ff_effects_max`]).
    ///
    /// LEDs and Sounds simply need to be triggered or toggled when encountering a matching
    /// [`LedEvent`] or [`SoundEvent`].
    ///
    /// Force feedback is a bit more involved:
    /// - An attempt to upload a force-feedback effect is signaled by a [`UinputEvent`] sent to the
    ///   [`UinputDevice`].
    /// - The uinput device then has to call [`UinputDevice::ff_upload`] to perform the upload.
    /// - The kernel driver will assign an [`EffectId`] to the effect (later used to start and stop
    ///   it) and make the uploaded effect data available to the uinput device.
    ///
    /// Effect deletion works the same way:
    /// - When a client wants to delete an effect, a [`UinputEvent`] will be sent to the device.
    /// - The device then has to call [`UinputDevice::ff_erase`] to perform the deletion.
    ///
    /// In both cases, the evdev client will block until [`UinputDevice::ff_upload`] or
    /// [`UinputDevice::ff_erase`] has been called.
    ///
    /// [`LedEvent`]: crate::event::LedEvent
    /// [`SoundEvent`]: crate::event::SoundEvent
    /// [`ForceFeedbackEvent`]: crate::event::ForceFeedbackEvent
    pub fn events(&self) -> Events<'_> {
        Events { dev: self }
    }

    /// Deprecated alias of [`UinputDevice::is_readable`].
    #[deprecated(note = "renamed to `is_readable`")]
    pub fn can_read(&self) -> io::Result<bool> {
        is_readable(self.as_raw_fd())
    }

    /// Returns whether this device has any pending events that can be read without blocking.
    ///
    /// If this returns `true`, calling [`UinputDevice::events()`] and then calling
    /// [`Events::next()`] is guaranteed to not block (but only for a single event).
    pub fn is_readable(&self) -> io::Result<bool> {
        is_readable(self.as_raw_fd())
    }

    /// Blocks the calling thread until [`UinputDevice::is_readable`] would return `true`.
    ///
    /// This works irrespective of whether `self` is in blocking or non-blocking mode.
    ///
    /// If `self` is already readable, this will return immediately.
    pub fn block_until_readable(&self) -> io::Result<()> {
        block_until_readable(self.as_raw_fd())
    }

    /// Performs a requested force-feedback effect upload.
    ///
    /// This should be called when receiving a [`UinputEvent`] with a code of
    /// [`UinputCode::FF_UPLOAD`].
    ///
    /// If `handler` returns an error, that error will both be returned to the caller of `ff_upload`
    /// and also to whichever process attempted to upload the effect.
    /// This requires a lossy conversion to a C style `Exyz` error constant.
    /// If `handler` returns a native OS error (eg. via [`io::Error::last_os_error`]), we'll return
    /// that error code directly.
    /// Otherwise, we'll try to translate the [`io::ErrorKind`] of the error to something sensible.
    pub fn ff_upload<R>(
        &self,
        request: &UinputEvent,
        handler: impl FnOnce(&ForceFeedbackUpload) -> io::Result<R>,
    ) -> io::Result<R> {
        assert!(request.code() == UinputCode::FF_UPLOAD);

        let mut upload = unsafe { mem::zeroed::<ForceFeedbackUpload>() };
        upload.0.request_id = request.raw_value() as u32;

        let now = Instant::now();
        let _d = on_drop(|| log::trace!("`ff_upload` took {:?}", now.elapsed()));
        unsafe {
            self.ioctl("UI_BEGIN_FF_UPLOAD", UI_BEGIN_FF_UPLOAD, &mut upload.0)?;
        }

        let res = handler(&upload);
        match &res {
            Ok(_) => {}
            Err(e) => {
                let os_err = e.raw_os_error();
                let errno = e
                    .raw_os_error()
                    .unwrap_or_else(|| errorkind2libc(e.kind()).unwrap_or(libc::EIO));
                log::debug!(
                    "ff_upload handler errored: {e} ({:?}, OS error: {os_err:?}) -> code {errno}",
                    e.kind()
                );
                upload.0.retval = -errno;
            }
        }

        unsafe {
            self.ioctl("UI_END_FF_UPLOAD", UI_END_FF_UPLOAD, &upload.0)?;
        }

        res
    }

    /// Performs a requested force-feedback effect erasure.
    ///
    /// This should be called when receiving a [`UinputEvent`] with a code of
    /// [`UinputCode::FF_ERASE`].
    ///
    /// If `handler` returns an error, that error will both be returned to the caller of `ff_erase`
    /// and also to whichever process attempted to upload the effect.
    /// This requires a lossy conversion to a C style `Exyz` error constant.
    /// If `handler` returns a native OS error (eg. via [`io::Error::last_os_error`]), we'll return
    /// that error code directly.
    /// Otherwise, we'll try to translate the [`io::ErrorKind`] of the error to something sensible.
    pub fn ff_erase(
        &self,
        request: &UinputEvent,
        handler: impl FnOnce(&ForceFeedbackErase) -> io::Result<()>,
    ) -> io::Result<()> {
        assert!(request.code() == UinputCode::FF_ERASE);

        let mut erase = unsafe { mem::zeroed::<ForceFeedbackErase>() };
        erase.0.request_id = request.raw_value() as u32;
        unsafe {
            self.ioctl("UI_BEGIN_FF_ERASE", UI_BEGIN_FF_ERASE, &mut erase.0)?;
        }

        match handler(&erase) {
            Ok(()) => {}
            Err(e) => {
                let os_err = e.raw_os_error();
                let errno = e
                    .raw_os_error()
                    .unwrap_or_else(|| errorkind2libc(e.kind()).unwrap_or(libc::EIO));
                log::debug!(
                    "ff_erase handler errored: {e} ({:?}, OS error: {os_err:?}) -> code {errno}",
                    e.kind()
                );
                erase.0.retval = -errno;
            }
        }

        unsafe {
            self.ioctl("UI_END_FF_ERASE", UI_END_FF_ERASE, &erase.0)?;
        }

        Ok(())
    }

    /// Writes a batch of input events to the device.
    ///
    /// The event batch will be automatically followed up by a `SYN_REPORT` event.
    ///
    /// # Kernel Processing
    ///
    /// The kernel will discard invalid events, events that don't correspond with an event type
    /// that was enabled during construction, as well as redundant events (whose value matches the
    /// current state of the button/axis).
    ///
    /// It will also set the event timestamp to the current time (at least it will do this if the
    /// time stamp is zero).
    ///
    /// [`RelEvent`]s will always be forwarded to readers, since there is no state associated with
    /// them.
    ///
    /// [`RelEvent`]: crate::event::RelEvent
    pub fn write(&self, events: &[InputEvent]) -> io::Result<()> {
        self.writer().write(events)?.finish()?;
        Ok(())
    }

    /// Returns an [`EventWriter`] for writing events to the device.
    ///
    /// Call [`EventWriter::finish`] to write a `SYN_REPORT` event and end the event batch.
    ///
    /// The same considerations as for [`UinputDevice::write`] apply to using the [`EventWriter`].
    pub fn writer(&self) -> EventWriter<'_> {
        EventWriter {
            file: &self.file,
            batch: BatchWriter::new(),
            needs_syn_report: true,
        }
    }
}

/// Helper for writing a sequence of events to the uinput device.
///
/// Returned by [`UinputDevice::writer`].
#[derive(Debug)]
#[must_use = "must call `EventWriter::finish` to flush the event batch"]
pub struct EventWriter<'a> {
    file: &'a File,
    batch: BatchWriter,
    needs_syn_report: bool,
}

impl<'a> EventWriter<'a> {
    /// Writes raw events to the device.
    ///
    /// Events passed to this method may be buffered to improve performance.
    pub fn write(mut self, events: &[InputEvent]) -> io::Result<Self> {
        self.batch.write(events, self.file)?;
        Ok(self)
    }

    /// Prepares for modification of a multi-touch slot.
    ///
    /// This will publish an `ABS_MT_SLOT` event with the selected slot.
    ///
    /// Returns an [`SlotWriter`] that can be used to modify `slot`.
    pub fn slot(mut self, slot: impl TryInto<Slot>) -> io::Result<SlotWriter<'a>> {
        let slot: Slot = slot
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid slot"))?;
        self = self.write(&[AbsEvent::new(Abs::MT_SLOT, slot.raw() as i32).into()])?;
        Ok(SlotWriter(self))
    }

    /// Changes the device's [`KeyRepeat`] configuration.
    ///
    /// Requires that [`Builder::with_key_repeat`] was called to enable the autorepeat
    /// functionality.
    ///
    /// This will write 2 [`RepeatEvent`]s: one with [`Repeat::PERIOD`] and one with
    /// [`Repeat::DELAY`]. The uinput system will immediately echo both events back to the
    /// [`UinputDevice`], and to every connected `evdev` client.
    pub fn set_key_repeat(self, rep: KeyRepeat) -> io::Result<Self> {
        self.write(&[
            RepeatEvent::new(Repeat::PERIOD, rep.period()).into(),
            RepeatEvent::new(Repeat::DELAY, rep.delay()).into(),
        ])
    }

    /// Finishes this batch of events by sending a `SYN_REPORT` event.
    pub fn finish(mut self) -> io::Result<()> {
        self.finish_impl()?;
        Ok(())
    }

    fn finish_impl(&mut self) -> io::Result<()> {
        if self.needs_syn_report {
            self.needs_syn_report = false;
            self.batch
                .write(&[SynEvent::new(Syn::REPORT).into()], self.file)?;
        }
        self.batch.flush(self.file)
    }
}
impl Drop for EventWriter<'_> {
    fn drop(&mut self) {
        // If this is called after `finish`, `needs_syn_report` will be `false`, and the call to
        // `self.batch.flush` will do nothing.
        if let Err(e) = self.finish_impl() {
            log::error!("uncaught error in `EventWriter` destructor: {e}");
        }
    }
}

/// Writes events to a selected multitouch slot.
///
/// Returned by [`EventWriter::slot`].
#[derive(Debug)]
#[must_use = "must call `SlotWriter::finish_slot` to finish modifying this slot"]
pub struct SlotWriter<'a>(EventWriter<'a>);

impl<'a> SlotWriter<'a> {
    /// Sets the X and Y positions of this MT slot.
    ///
    /// This will emit [`Abs::MT_POSITION_X`] and [`Abs::MT_POSITION_Y`] events.
    pub fn set_position(mut self, x: i32, y: i32) -> io::Result<Self> {
        self.0 = self.0.write(&[
            AbsEvent::new(Abs::MT_POSITION_X, x).into(),
            AbsEvent::new(Abs::MT_POSITION_Y, y).into(),
        ])?;
        Ok(self)
    }

    /// Set the tracking ID of this MT slot.
    pub fn set_tracking_id(mut self, id: i32) -> io::Result<Self> {
        self.0 = self
            .0
            .write(&[AbsEvent::new(Abs::MT_TRACKING_ID, id).into()])?;
        Ok(self)
    }

    /// Write raw events to the device.
    ///
    /// Any `ABS_MT_*` events will be associated with this MT slot.
    pub fn write(mut self, events: &[InputEvent]) -> io::Result<Self> {
        self.0 = self.0.write(events)?;
        Ok(self)
    }

    /// Finishes updating this multitouch slot and returns the original [`EventWriter`].
    pub fn finish_slot(self) -> io::Result<EventWriter<'a>> {
        Ok(self.0)
    }
}

/// An iterator over the events received by a [`UinputDevice`].
///
/// If the [`UinputDevice`] is in non-blocking mode, this iterator will end when there are no more
/// events to read without blocking.
/// Otherwise, iteration will block until more events are available.
///
/// [`UinputDevice::try_clone`] may be used to create multiple handles to the same device, so that
/// one thread can read events while another writes them.
#[derive(Debug)]
pub struct Events<'a> {
    dev: &'a UinputDevice,
}

impl Iterator for Events<'_> {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let mut dest = MaybeUninit::<InputEvent>::uninit();
            let bptr = dest.as_mut_ptr().cast::<u8>();
            let byte_buf = slice::from_raw_parts_mut(bptr, mem::size_of::<InputEvent>());
            let bytes = match (&self.dev.file).read(byte_buf) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return None,
                Err(e) => return Some(Err(e)),
            };
            assert_eq!(bytes, mem::size_of::<InputEvent>());
            Some(Ok(dest.assume_init()))
        }
    }
}

/// Contains data about a force-feedback effect upload or update.
///
/// See [`UinputDevice::ff_upload`].
#[repr(transparent)]
pub struct ForceFeedbackUpload(uinput_ff_upload);

impl ForceFeedbackUpload {
    /// Returns the [`Effect`] that is being uploaded.
    pub fn effect(&self) -> &Effect<'static> {
        // Safety: `#[repr(transparent)]`
        unsafe { mem::transmute::<&ff_effect, &Effect>(&self.0.effect) }
    }

    /// Returns the [`EffectId`] the input system has assigned to this force-feedback effect.
    ///
    /// This ID is referenced by force-feedback trigger events and by [`ForceFeedbackErase`]
    /// commands, so implementations should store this somewhere.
    pub fn effect_id(&self) -> EffectId {
        self.effect().id()
    }

    /// If this upload overwrites an existing [`Effect`], this returns that effect.
    ///
    /// If this upload is uploading a *new* [`Effect`], this will refer to an invalid [`Effect`]
    /// structure (likely with all fields zeroed out).
    pub fn old(&self) -> &Effect<'static> {
        // Safety: `#[repr(transparent)]`
        unsafe { mem::transmute::<&ff_effect, &Effect>(&self.0.old) }
    }
}

impl fmt::Debug for ForceFeedbackUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForceFeedbackUpload")
            .field("request_id", &self.0.request_id)
            .field("effect", self.effect())
            .field("old", self.old())
            .finish()
    }
}

/// Contains data about a force-feedback effect deletion.
///
/// See [`UinputDevice::ff_erase`].
#[repr(transparent)]
pub struct ForceFeedbackErase(uinput_ff_erase);

impl ForceFeedbackErase {
    pub fn effect_id(&self) -> EffectId {
        EffectId(self.0.effect_id.try_into().unwrap())
    }
}

impl fmt::Debug for ForceFeedbackErase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForceFeedbackErase")
            .field("request_id", &self.0.request_id)
            .field("effect_id", &self.effect_id())
            .finish()
    }
}
