use std::{
    hash::{BuildHasher, Hasher, RandomState},
    io,
    iter::zip,
};

use crate::{
    Evdev,
    event::{EventType, InputEvent},
    hotplug::HotplugMonitor,
    uinput::{Builder, UinputDevice},
};

/// Creates a [`UinputDevice`] and [`Evdev`] that are connected to each other.
#[allow(dead_code)]
pub fn pair(b: impl FnOnce(Builder) -> io::Result<Builder>) -> io::Result<(UinputDevice, Evdev)> {
    fn hash() -> u64 {
        RandomState::new().build_hasher().finish()
    }

    let hash = hash();
    let name = format!("-@-rust-evdevil-device-{hash}-@-");

    let hotplug = HotplugMonitor::new()?;
    let uinput = b(UinputDevice::builder()?)?.build(&name)?;
    for res in hotplug {
        match res {
            Ok(evdev) => {
                if evdev.name()? == name {
                    return Ok((uinput, evdev));
                }
            }
            Err(e) => {
                // This can happen when an unrelated device (likely created by another test) shows
                // up first, and disappears before we can check its name.
                log::warn!("got error while waiting for '{name}' to appear: {e}");
            }
        }
    }
    unreachable!("hotplug event stream should be infinite")
}

pub fn events_eq(recv: InputEvent, expected: InputEvent) -> bool {
    if recv.event_type() != expected.event_type() || recv.raw_code() != expected.raw_code() {
        return false;
    }

    // Value is ignored for SYN events
    if recv.event_type() != EventType::SYN && recv.raw_value() != expected.raw_value() {
        return false;
    }
    true
}

#[track_caller]
pub fn check_events(
    actual: impl IntoIterator<Item = InputEvent>,
    expected: impl IntoIterator<Item = InputEvent>,
) {
    let actual: Vec<_> = actual.into_iter().collect();
    let expected: Vec<_> = expected.into_iter().collect();
    assert_eq!(
        actual.len(),
        expected.len(),
        "expected {} events, got {actual:?}",
        expected.len()
    );
    if !zip(actual.iter().copied(), expected.iter().copied()).all(|(a, b)| events_eq(a, b)) {
        panic!("expected {expected:?}, got {actual:?}");
    }
}
