#![allow(dead_code)]

use std::{
    fmt,
    hash::{BuildHasher, Hasher, RandomState},
    io,
    iter::zip,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    Evdev,
    event::{EventType, InputEvent},
    hotplug::HotplugMonitor,
    uinput::{Builder, UinputDevice},
};

fn hash() -> u64 {
    RandomState::new().build_hasher().finish()
}

/// Creates a [`UinputDevice`] and [`Evdev`] that are connected to each other.
pub fn pair(b: impl FnOnce(Builder) -> io::Result<Builder>) -> io::Result<(UinputDevice, Evdev)> {
    let hash = hash();
    let name = format!("-@-rust-evdevil-device-{hash}-@-");

    let hotplug = HotplugMonitor::new()?;
    let uinput = b(UinputDevice::builder()?)?.build(&name)?;
    for res in hotplug {
        let evdev = res?;
        if evdev.name()? == name {
            return Ok((uinput, evdev));
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

/// A `Future` that polls its argument once and panics unless the inner poll results in `Pending`.
pub struct AssertPending<'a, F>(pub Pin<&'a mut F>);

impl<'a, F: Future> Future for AssertPending<'a, F>
where
    F::Output: fmt::Debug,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.as_mut().poll(cx) {
            Poll::Ready(val) => panic!("expected `Pending`, got `Ready`: {val:?}"),
            Poll::Pending => Poll::Ready(()),
        }
    }
}
