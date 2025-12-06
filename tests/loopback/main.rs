//! The loopback test.
//!
//! This test makes up the main test suite of this library.
//!
//! It creates a `uinput` device and opens its `evdev` end, and then performs a variety of
//! operations on one end, verifying the result on the other.
//!
//! Since there is kernel state, all tests have to be run in series. The [`Tester::get()`] method
//! ensures that access is only possible from one test at a time, and also makes sure that each test
//! leaves the device in a pristine state, so as to not affect the subsequent tests.

mod clock;
mod events;
mod ff;
mod mt;
mod repeat;
mod revoke;

use std::{
    fmt, fs,
    hash::{BuildHasher, Hasher, RandomState},
    io,
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    panic::resume_unwind,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread::{self, JoinHandle},
    time::Duration,
};

use evdevil::{
    AbsInfo, Bus, Evdev, EventReader, InputId, InputProp, KeyRepeat,
    bits::{BitSet, BitValue},
    event::{Abs, EventType, Key, Led, Misc, Rel, Sound, Switch},
    ff::Feature,
    uinput::{AbsSetup, UinputDevice},
};

const TEST_DEVICE_NAME: &str = "-@-rust-loopback-test-@-";

fn setup() -> io::Result<Tester> {
    env_logger::builder()
        .filter_module(env!("CARGO_PKG_NAME"), log::LevelFilter::Trace)
        .init();

    let uinput = setup_uinput_device()?;

    let mut dev = None;
    let mut retries = 5;
    'outer: while retries > 0 {
        retries -= 1;

        for res in evdevil::enumerate()? {
            let res = || -> io::Result<Option<_>> {
                let evdev = res?;
                if evdev.name()? == TEST_DEVICE_NAME {
                    Ok(Some(evdev))
                } else {
                    Ok(None)
                }
            }();

            match res {
                Ok(None) => {}
                Ok(Some(reader)) => {
                    dev = Some(reader);
                    break 'outer;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                }
            }
        }

        thread::sleep(Duration::from_millis(150));
        println!("(retrying)");
    }

    let evdev = dev.expect("could not find test device");
    println!(
        "opened test device '{}' at '{}'",
        evdev.name()?,
        evdev.path().display(),
    );

    assert!(!evdev.is_readable()?);

    Ok(Tester {
        uinput,
        evdev: Some(evdev),
        bg: None,
    })
}

struct Tester {
    uinput: UinputDevice,
    evdev: Option<Evdev>,
    bg: Option<(Arc<Mutex<Option<Evdev>>>, JoinHandle<io::Result<()>>)>,
}

static TESTER: Mutex<Option<Tester>> = Mutex::new(None);

impl Tester {
    /// Returns a mutex guard, because the tests have to run in sequence.
    fn get() -> impl DerefMut<Target = Tester> {
        struct TesterHandle(MutexGuard<'static, Option<Tester>>);
        impl Drop for TesterHandle {
            fn drop(&mut self) {
                if thread::panicking() {
                    return;
                }

                if self.bg.is_some() {
                    self.join_thread();
                }

                // Ensure that every test leaves the device in a pristine state.
                let mut pending = Vec::new();
                while self.evdev().is_readable().unwrap() {
                    pending.push(self.evdev().raw_events().next().unwrap());
                }
                assert!(
                    pending.is_empty(),
                    "{} pending events left in evdev buffer: {:?}",
                    pending.len(),
                    pending
                );

                let mut pending = Vec::new();
                while self.uinput.is_readable().unwrap() {
                    pending.push(self.uinput.events().next().unwrap());
                }
                assert!(
                    pending.is_empty(),
                    "{} pending events left in uinput buffer: {:?}",
                    pending.len(),
                    pending
                );

                assert_eq!(self.evdev().key_state().unwrap(), BitSet::new());
                assert_eq!(self.evdev().switch_state().unwrap(), BitSet::new());
                assert_eq!(self.evdev().led_state().unwrap(), BitSet::new());
                assert_eq!(self.evdev().sound_state().unwrap(), BitSet::new());
                assert_eq!(self.evdev().abs_info(Abs::BRAKE).unwrap(), ABS_INFO_BRAKE);
                assert_eq!(
                    self.evdev().abs_info(Abs::MT_SLOT).unwrap(),
                    ABS_INFO_MT_SLOTS
                );
                assert_eq!(self.evdev().key_repeat().unwrap(), Some(KEY_REPEAT));
                assert_eq!(self.evdev().set_nonblocking(false).unwrap(), false);
                if cfg!(target_os = "linux") {
                    // (on FreeBSD this fails with EINVAL)
                    assert_eq!(self.uinput.set_nonblocking(false).unwrap(), false);
                }
                // NB: does not check multitouch slot states, due to lack of evdevil API
            }
        }
        impl Deref for TesterHandle {
            type Target = Tester;

            fn deref(&self) -> &Self::Target {
                self.0.as_ref().unwrap()
            }
        }
        impl DerefMut for TesterHandle {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.0.as_mut().unwrap()
            }
        }

        // Shuffle the test order a bit:
        let rng = RandomState::new().build_hasher().finish();
        let micros = rng % 50_000;
        thread::sleep(Duration::from_micros(micros));

        let mut guard = match TESTER.lock() {
            Ok(g) => g,
            Err(_poison) => {
                // Hide the backtrace / libtest noise; the causing thread already printed everything helpful.
                resume_unwind(Box::new("(silent unwind due to poison error)"))
            }
        };
        if guard.is_none() {
            *guard = Some(setup().unwrap());
        }
        TesterHandle(guard)
    }

    fn evdev(&self) -> &Evdev {
        self.evdev.as_ref().unwrap()
    }

    fn evdev_mut(&mut self) -> &mut Evdev {
        self.evdev.as_mut().unwrap()
    }

    fn with_reader<R>(
        &mut self,
        cb: impl FnOnce(&mut UinputDevice, &mut EventReader) -> io::Result<R>,
    ) -> io::Result<R> {
        let dev = self.evdev.take().unwrap();
        let mut reader = dev.into_reader().expect("failed to create `EventReader`");
        let res = cb(&mut self.uinput, &mut reader);
        self.evdev = Some(reader.into_evdev());
        res
    }

    /// Schedules `thread` to happen in a background thread.
    #[cfg_attr(target_os = "freebsd", expect(dead_code))]
    fn with_evdev_thread(
        &mut self,
        thread: impl FnOnce(&mut Evdev) -> io::Result<()> + Send + 'static,
    ) {
        assert!(self.bg.is_none());

        let evdev = self.evdev.take().unwrap();
        let mtx = Arc::new(Mutex::new(Some(evdev)));
        let mtx2 = mtx.clone();
        let handle = thread::spawn(move || {
            let mut guard = mtx2.lock().unwrap();
            match thread(guard.as_mut().unwrap()) {
                Ok(()) => Ok(()),
                Err(e) => {
                    log::error!("error from evdev thread: {e}");
                    Err(e)
                }
            }
        });
        self.bg = Some((mtx, handle));
    }

    fn join_thread(&mut self) {
        let (mtx, bg) = self.bg.take().unwrap();

        println!("waiting for background operation to complete");
        match bg.join() {
            Ok(res) => res.unwrap(),
            Err(payload) => resume_unwind(payload),
        }
        self.evdev = Some(
            mtx.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .unwrap(),
        );
    }
}

const INPUT_ID: InputId = InputId::new(Bus::VIRTUAL, 9876, 12345, 1010);

const PHYS: &str = "blablablaPHYS";

const PROPS: &[InputProp] = &[InputProp::BUTTONPAD];

// We use only "obscure" button codes since the desktop environment might otherwise input the test
// events into whatever application has focus.
const KEYS: &[Key] = &[Key::BTN_TRIGGER_HAPPY1, Key::BTN_TRIGGER_HAPPY2];

const REL: &[Rel] = &[Rel::DIAL];

const MISC: &[Misc] = &[Misc::GESTURE, Misc::SCAN];

const LEDS: &[Led] = &[Led::CAPSL];

const SOUNDS: &[Sound] = &[Sound::BELL];

const SWITCHES: &[Switch] = &[Switch::CAMERA_LENS_COVER];

const MT_SLOTS: u16 = 4;
const ABS_INFO_BRAKE: AbsInfo = AbsInfo::new(-100, 100)
    .with_flat(4)
    .with_fuzz(5)
    .with_resolution(15);
const ABS_INFO_MT_SLOTS: AbsInfo = AbsInfo::new(0, MT_SLOTS as i32);
const ABS: &[AbsSetup] = &[
    AbsSetup::new(Abs::BRAKE, ABS_INFO_BRAKE),
    AbsSetup::new(Abs::MT_SLOT, ABS_INFO_MT_SLOTS),
    AbsSetup::new(Abs::MT_POSITION_X, AbsInfo::new(-1000, 1000)),
    AbsSetup::new(Abs::MT_POSITION_Y, AbsInfo::new(-1000, 1000)),
    AbsSetup::new(Abs::MT_TRACKING_ID, AbsInfo::new(-1, i32::MAX)),
    // Fun fact: the kernel will use a larger event buffer size if `ABS_MT_POSITION_X` is enabled.
    // This requires adjustments in the overflow tests.
];

const KEY_REPEAT: KeyRepeat = KeyRepeat::new(333, 50);

const FF_EFFECTS: u32 = 2;
const FF_FEATURES: &[Feature] = &[Feature::RUMBLE];

fn setup_uinput_device() -> io::Result<UinputDevice> {
    // Create a Buddhist computer peripheral (one with everything)
    let dev = UinputDevice::builder()?
        .with_input_id(INPUT_ID)?
        .with_ff_effects_max(FF_EFFECTS)?
        .with_ff_features(FF_FEATURES.iter().copied())?
        .with_phys(PHYS)?
        .with_props(PROPS.iter().copied())?
        .with_keys(KEYS.iter().copied())?
        .with_rel_axes(REL.iter().copied())?
        .with_misc(MISC.iter().copied())?
        .with_leds(LEDS.iter().copied())?
        .with_sounds(SOUNDS.iter().copied())?
        .with_switches(SWITCHES.iter().copied())?
        .with_abs_axes(ABS.iter().copied())?
        .with_key_repeat()?
        .build(TEST_DEVICE_NAME)?;

    // Key repeat is set by writing `KeyRepeat` events to the stream.
    dev.writer().set_key_repeat(KEY_REPEAT)?.finish()?;
    // The events are echoed right back at us, but not on FreeBSD.
    if !cfg!(target_os = "freebsd") {
        dev.events().next().unwrap()?;
        dev.events().next().unwrap()?;
    }

    Ok(dev)
}

#[test]
fn test_device_id() -> io::Result<()> {
    let tester = Tester::get();
    let devid = tester.evdev().input_id()?;
    assert_eq!(devid.bus(), INPUT_ID.bus());
    assert_eq!(devid.vendor(), INPUT_ID.vendor());
    assert_eq!(devid.product(), INPUT_ID.product());
    assert_eq!(devid.version(), INPUT_ID.version());
    Ok(())
}

#[test]
#[cfg_attr(target_os = "freebsd", ignore = "unsupported (always 0) on FreeBSD")]
fn test_ff_limit() -> io::Result<()> {
    let limit = Tester::get().evdev().supported_ff_effects()?;
    assert_eq!(limit, FF_EFFECTS);
    Ok(())
}

#[test]
fn test_phys() -> io::Result<()> {
    let tester = Tester::get();
    assert_eq!(tester.evdev().phys()?.unwrap(), PHYS);
    Ok(())
}

#[test]
fn test_props() -> io::Result<()> {
    let tester = Tester::get();
    assert_eq!(tester.evdev().props()?, PROPS.iter().copied().collect());
    Ok(())
}

#[test]
fn test_uniq() -> io::Result<()> {
    // There's no ioctl to set this.
    let uniq = Tester::get().evdev().unique_id()?;
    assert_eq!(uniq, None);
    Ok(())
}

#[test]
fn test_sysname() -> io::Result<()> {
    let t = Tester::get();
    let mut path = PathBuf::from("/sys/devices/virtual/input/");
    let sysname = t.uinput.sysname()?;
    path.push(sysname);

    println!("uinput sys path: {}", path.display());
    println!("evdev path: {}", t.evdev().path().display());

    if cfg!(target_os = "linux") {
        for res in fs::read_dir(&path)? {
            let entry = res?;
            if entry.file_name().as_bytes().starts_with(b"event") {
                let mut devpath = PathBuf::from("/dev/input/");
                devpath.push(entry.file_name());

                assert_eq!(devpath, t.evdev().path());
            }
        }
    }

    Ok(())
}

#[test]
fn test_advertised_event_codes() -> io::Result<()> {
    fn check<V>(actual: BitSet<V>, expected: &[V])
    where
        V: BitValue + PartialEq + fmt::Debug,
    {
        let supported = actual.iter().collect::<Vec<_>>();
        assert_eq!(supported, expected);
    }

    let t = Tester::get();
    assert_eq!(
        t.evdev().supported_events()?,
        BitSet::from_iter([
            #[cfg(not(target_os = "freebsd"))] // FreeBSD omits the implicit SYN event
            EventType::SYN,
            EventType::KEY,
            EventType::REL,
            EventType::ABS,
            EventType::MSC,
            EventType::SW,
            EventType::LED,
            EventType::SND,
            EventType::REP,
            EventType::FF,
        ])
    );
    check(t.evdev().supported_keys()?, KEYS);
    check(t.evdev().supported_rel_axes()?, REL);
    check(t.evdev().supported_misc()?, MISC);
    check(t.evdev().supported_leds()?, LEDS);
    check(t.evdev().supported_sounds()?, SOUNDS);
    check(t.evdev().supported_switches()?, SWITCHES);

    let expected = ABS.iter().map(|setup| setup.abs()).collect::<Vec<_>>();
    let actual = t
        .evdev()
        .supported_abs_axes()?
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(expected, actual);

    if !cfg!(target_os = "freebsd") {
        assert_eq!(t.evdev().supported_ff_effects()?, FF_EFFECTS);
        check(t.evdev().supported_ff_features()?, FF_FEATURES);
    }

    Ok(())
}

#[test]
fn test_abs_info() -> io::Result<()> {
    let mut t = Tester::get();

    // Should be `ABS_INFO_BRAKE` by default.
    let info = t.evdev().abs_info(Abs::BRAKE)?;
    assert_eq!(info, ABS_INFO_BRAKE);

    let new_info = AbsInfo::new(
        ABS_INFO_BRAKE.minimum() + 300,
        ABS_INFO_BRAKE.maximum() - 200,
    )
    .with_raw_value(-123);
    t.evdev_mut().set_abs_info(Abs::BRAKE, new_info)?;
    let info = t.evdev().abs_info(Abs::BRAKE)?;
    assert_eq!(info, new_info);

    t.evdev_mut().set_abs_info(Abs::BRAKE, ABS_INFO_BRAKE)?;
    let info = t.evdev().abs_info(Abs::BRAKE)?;
    assert_eq!(info, ABS_INFO_BRAKE);

    Ok(())
}
