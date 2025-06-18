use std::{
    io,
    iter::zip,
    thread,
    time::{Duration, SystemTime},
};

use evdevil::{
    Evdev,
    bits::BitSet,
    event::{
        Abs, AbsEvent, EventKind, EventType, InputEvent, Key, KeyEvent, KeyState, Led, LedEvent,
        Rel, RelEvent, Syn, SynEvent,
    },
    uinput::UinputDevice,
};

use crate::Tester;

/// Sends `events` to the `uinput` device, and asserts that they arrive at the `evdev`.
#[track_caller]
fn roundtrip_raw(t: &mut Tester, events: &[InputEvent]) -> io::Result<()> {
    roundtrip_raw2(t.evdev.as_ref().unwrap(), &mut t.uinput, events)
}

#[track_caller]
fn roundtrip_raw2(
    evdev: &Evdev,
    uinput: &mut UinputDevice,
    events: &[InputEvent],
) -> io::Result<()> {
    uinput.write(events)?;

    for expected in events {
        let recv = evdev.raw_events().next().unwrap()?;
        if !events_eq(&recv, expected) {
            panic!("expected {expected:?} in evdev, got {recv:?}");
        }
    }
    let syn = evdev.raw_events().next().unwrap()?;
    assert_eq!(
        syn.event_type(),
        EventType::SYN,
        "expected SYN from evdev, got {syn:?}"
    );

    Ok(())
}

#[track_caller]
fn roundtrip_echo(t: &mut Tester, events: &[InputEvent]) -> io::Result<()> {
    roundtrip_raw(t, events)?;

    assert!(t.uinput.can_read()?);
    for expected in events {
        let recv = t.uinput.events().next().unwrap()?;
        if !events_eq(&recv, expected) {
            panic!("expected {expected:?} in uinput device, got {recv:?}");
        }
    }
    Ok(())
}

#[track_caller]
fn evdev2uinput(t: &mut Tester, events: &[InputEvent]) -> io::Result<()> {
    t.evdev_mut().write(events)?;

    if !events.is_empty() {
        assert!(t.uinput.can_read()?);
    }
    for expected in events {
        let recv = t.uinput.events().next().unwrap()?;
        if !events_eq(&recv, expected) {
            panic!("expected {expected:?} in uinput device, got {recv:?}");
        }
    }
    if t.uinput.can_read()? {
        panic!("found pending event: {:?}", t.uinput.events().next());
    }

    Ok(())
}

fn events_eq(recv: &InputEvent, expected: &InputEvent) -> bool {
    if recv.event_type() != expected.event_type() || recv.raw_code() != expected.raw_code() {
        return false;
    }

    // Value is ignored for SYN events
    if recv.event_type() != EventType::SYN && recv.raw_value() != expected.raw_value() {
        return false;
    }
    true
}

fn check_events(actual: &[InputEvent], expected: &[InputEvent]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "expected {} events, got {actual:?}",
        expected.len()
    );
    if !zip(actual.iter(), expected.iter()).all(|(a, b)| events_eq(a, b)) {
        panic!("expected {expected:?}, got {actual:?}");
    }
}

#[test]
fn test_can_read() -> io::Result<()> {
    let tester = Tester::get();

    assert!(!tester.evdev().can_read()?);

    let event = RelEvent::new(Rel::DIAL, -42).into();
    tester.uinput.write(&[event])?;

    assert!(tester.evdev().can_read()?);
    let recv = tester.evdev().raw_events().next().unwrap()?;
    if !events_eq(&recv, &event) {
        panic!("expected {event:?}, got {recv:?}");
    }

    // `EV_SYN`
    assert!(tester.evdev().can_read()?);
    let ev = tester.evdev().raw_events().next().unwrap()?;
    assert_eq!(ev.event_type(), EventType::SYN);

    assert!(
        !tester.evdev().can_read()?,
        "unexpected pending event: {:?}",
        tester.evdev().raw_events().next()
    );

    Ok(())
}

/// Tests that pressing and releasing a single key updates the kernel-side state and lets us receive
/// the event.
#[test]
fn test_single_key_event() -> io::Result<()> {
    let mut tester = Tester::get();

    assert_eq!(tester.evdev().key_state()?, BitSet::new());

    roundtrip_raw(
        &mut tester,
        &[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED).into()],
    )?;

    assert_eq!(
        tester.evdev().key_state()?,
        BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1])
    );

    roundtrip_raw(
        &mut tester,
        &[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED).into()],
    )?;
    assert_eq!(tester.evdev().key_state()?, BitSet::new());

    Ok(())
}

#[test]
fn test_led() -> io::Result<()> {
    let mut tester = Tester::get();

    // evdev -> uinput
    evdev2uinput(&mut tester, &[LedEvent::new(Led::CAPSL, true).into()])?;
    assert_eq!(tester.evdev().led_state()?, BitSet::from_iter([Led::CAPSL]));
    evdev2uinput(&mut tester, &[LedEvent::new(Led::CAPSL, false).into()])?;
    assert_eq!(tester.evdev().led_state()?, BitSet::new());

    thread::sleep(Duration::from_millis(50));
    assert!(!tester.evdev().can_read()?);
    assert!(!tester.uinput.can_read()?);

    // For some reason, the kernel will insert "LED on" and "LED off" events before the next event
    // the uinput device emits, so drain them here.
    tester.uinput.write(&[RelEvent::new(Rel::DIAL, 7).into()])?;
    let mut ev = Vec::new();
    while tester.evdev().can_read()? {
        ev.push(tester.evdev().raw_events().next().unwrap()?);
    }
    eprintln!("draining events: {ev:?}");

    // uinput -> evdev
    // If `uinput` sends an LED or SND event, the kernel will also echo it back to the `uinput` device.
    // Note that the `uinput` event buffer does not make use of the `SYN_*` mechanism.
    tester
        .uinput
        .write(&[LedEvent::new(Led::CAPSL, true).into()])?;

    roundtrip_echo(&mut tester, &[LedEvent::new(Led::CAPSL, true).into()])?;
    assert_eq!(tester.evdev().led_state()?, BitSet::from_iter([Led::CAPSL]));
    roundtrip_echo(&mut tester, &[LedEvent::new(Led::CAPSL, false).into()])?;
    assert_eq!(tester.evdev().led_state()?, BitSet::new());

    thread::sleep(Duration::from_millis(50));
    assert!(!tester.evdev().can_read()?);
    assert!(!tester.uinput.can_read()?);

    Ok(())
}

#[test]
fn test_abs_events() -> io::Result<()> {
    let mut tester = Tester::get();

    roundtrip_raw(&mut tester, &[AbsEvent::new(Abs::BRAKE, 100).into()])?;
    assert_eq!(tester.evdev().abs_info(Abs::BRAKE)?.value(), 100);

    roundtrip_raw(&mut tester, &[AbsEvent::new(Abs::BRAKE, 0).into()])?;
    assert_eq!(tester.evdev().abs_info(Abs::BRAKE)?.value(), 0);

    Ok(())
}

/// Tests that `EventReader` will fetch the current device state when created, and that it will emit
/// synthetic events to synchronize whoever consumes those events.
#[test]
fn test_reader_init_sync() -> io::Result<()> {
    let mut t = Tester::get();

    // Press the key and make sure the `EventReader` emits an event to sync even if the event is not
    // in the kernel queue anymore.
    roundtrip_raw(
        &mut t,
        &[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED).into()],
    )?;

    t.evdev().set_nonblocking(true)?;
    t.with_reader(|_, reader| {
        let events = reader.events().collect::<io::Result<Vec<_>>>()?;
        check_events(
            &events,
            &[
                *KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED),
                Syn::REPORT.into(),
            ],
        );

        Ok(())
    })?;
    t.evdev().set_nonblocking(false)?;

    roundtrip_raw(
        &mut t,
        &[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED).into()],
    )?;

    Ok(())
}

/// Events to send to overflow the kernel buffer.
///
/// On Linux, the buffer appears to be around 128 events large.
/// Send an odd number of events to ensure any power-of-two shenanigans are partially filled.
const OVERFLOW_COUNT: usize = 555;

/// Tests that the kernel buffer will overflow and drop events as expected.
#[test]
fn test_overflow() -> io::Result<()> {
    let tester = Tester::get();

    // Use REL events, since they don't get processed, deduplicated, or ignored by the kernel.
    // Kernel buffer appears to be around 128 events large.
    let events = vec![RelEvent::new(Rel::DIAL, 1).into(); OVERFLOW_COUNT];
    tester.uinput.write(&events)?;

    assert!(tester.evdev().can_read()?);

    tester.evdev().set_nonblocking(true)?;
    let mut count = 0;
    loop {
        let event = match tester.evdev().raw_events().next().unwrap() {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                panic!("kernel buffer did not overflow (received {count} events)")
            }
            Err(e) => return Err(e),
        };
        match event.kind() {
            Some(EventKind::Rel(e)) if e.rel() == Rel::DIAL => count += 1,
            Some(EventKind::Syn(e)) if e.syn() == Syn::REPORT => count += 1,
            Some(EventKind::Syn(e)) if e.syn() == Syn::DROPPED => {
                println!("{count} events before SYN_DROPPED");
                assert!(count < OVERFLOW_COUNT);
                break;
            }
            _ => {
                panic!("unexpected event after {count} expected ones: {event:?}");
            }
        }
    }

    for res in tester.evdev().raw_events() {
        match res {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e),
        }
    }
    tester.evdev().set_nonblocking(false)?;

    Ok(())
}

/// Tests that `EventReader` will correctly resync when events are dropped.
#[test]
fn test_overflow_resync() -> io::Result<()> {
    let mut tester = Tester::get();

    // Start out with the key pressed.
    roundtrip_raw(
        &mut tester,
        &[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED).into()],
    )?;

    tester.with_reader(|uinput, reader| {
        assert_eq!(
            reader.evdev().key_state()?,
            BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1])
        );

        // Discard the initial events.
        reader.update()?;

        // Release the key without overflowing.
        uinput.write(&[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED).into()])?;
        // Update via next
        reader.evdev().set_nonblocking(true)?;
        let events = reader.events().collect::<io::Result<Vec<_>>>()?;
        check_events(
            &events,
            &[
                *KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED),
                *SynEvent::new(Syn::REPORT),
            ],
        );
        reader.evdev().set_nonblocking(false)?;
        assert_eq!(reader.evdev().key_state()?, BitSet::new());
        assert_eq!(reader.key_state(), &BitSet::new());

        // Press the key without overflowing.
        uinput.write(&[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED).into()])?;
        // Update via `update`
        reader.update()?;
        assert_eq!(
            reader.evdev().key_state()?,
            BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1])
        );
        assert_eq!(
            reader.key_state(),
            &BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1])
        );

        // Overflow the buffer, release the key, then overflow it again.
        let events = vec![RelEvent::new(Rel::DIAL, 1).into(); OVERFLOW_COUNT];
        uinput.write(&events)?;

        uinput.write(&[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED).into()])?;

        let events = vec![RelEvent::new(Rel::DIAL, 1).into(); OVERFLOW_COUNT];
        uinput.write(&events)?;

        // Kernel key state should now be released:
        assert_eq!(reader.evdev().key_state()?, BitSet::new());
        // But reader key state should still be pressed:
        assert_eq!(
            reader.key_state(),
            &BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1])
        );

        // Now use the reader to pull events until we see the key press.
        log::info!("waiting until we see the key press (should complete instantly)");
        reader.evdev().set_nonblocking(true)?;
        let mut ev = Vec::new();
        for res in &mut *reader {
            let event = match res {
                Ok(ev) => ev,
                Err(e) => {
                    reader.evdev().set_nonblocking(false)?;
                    return Err(e);
                }
            };
            if event.event_type() == EventType::REL {
                continue;
            }
            ev.push(event);
        }
        reader.evdev().set_nonblocking(false)?;
        check_events(
            &ev,
            &[
                *KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED),
                *SynEvent::new(Syn::REPORT),
                *SynEvent::new(Syn::REPORT),
            ],
        );

        // All events should have non-zero timestamps that monotonically increase
        for win in ev.windows(2) {
            let [a, b] = win else { panic!() };
            assert_ne!(a.time(), SystemTime::UNIX_EPOCH);
            assert_ne!(b.time(), SystemTime::UNIX_EPOCH);
            assert!(a.time() <= b.time());
        }

        // Reader key state should now be up-to-date
        assert_eq!(reader.key_state(), &BitSet::new());

        // Empty the rest of the kernel buffer.
        reader.update()?;

        Ok(())
    })?;

    Ok(())
}

#[test]
fn test_event_mask_state() -> io::Result<()> {
    let mut tester = Tester::get();

    let event_mask = tester.evdev().event_mask()?;

    tester.evdev_mut().set_event_mask(&BitSet::new())?;

    tester
        .uinput
        .write(&[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::PRESSED).into()])?;
    let keys = tester.evdev().key_state()?;
    assert_eq!(keys, BitSet::from_iter([Key::BTN_TRIGGER_HAPPY1]));

    tester
        .uinput
        .write(&[KeyEvent::new(Key::BTN_TRIGGER_HAPPY1, KeyState::RELEASED).into()])?;

    tester.evdev_mut().set_event_mask(&event_mask)?;
    Ok(())
}

#[test]
fn test_event_mask() -> io::Result<()> {
    let mut tester = Tester::get();

    let event_mask = tester.evdev().event_mask()?;

    tester
        .evdev_mut()
        .set_event_mask(&BitSet::from_iter([EventType::REL]))?;
    assert_eq!(
        tester.evdev().event_mask()?,
        BitSet::from_iter([EventType::REL]),
    );

    roundtrip_raw(&mut tester, &[RelEvent::new(Rel::DIAL, 1).into()])?;

    tester.evdev_mut().set_event_mask(&BitSet::new())?;
    assert_eq!(tester.evdev().event_mask()?, BitSet::new());
    assert!(!tester.evdev().can_read()?);

    // `REL_DIAL` events shouldn't arrive.
    tester.uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;
    assert!(!tester.evdev().can_read()?);

    tester.evdev_mut().set_event_mask(&event_mask)?;
    Ok(())
}

#[test]
fn test_rel_mask() -> io::Result<()> {
    let mut tester = Tester::get();

    let rel_mask = tester.evdev().rel_mask()?;
    assert!((0..=Rel::MAX.raw()).all(|rel| rel_mask.contains(Rel::from_raw(rel))));

    tester
        .evdev_mut()
        .set_rel_mask(&BitSet::from_iter([Rel::DIAL]))?;
    assert_eq!(tester.evdev().rel_mask()?, BitSet::from_iter([Rel::DIAL]),);

    roundtrip_raw(&mut tester, &[RelEvent::new(Rel::DIAL, 1).into()])?;

    tester.evdev_mut().set_rel_mask(&BitSet::new())?;
    assert_eq!(tester.evdev().rel_mask()?, BitSet::new());
    assert!(!tester.evdev().can_read()?);

    // `REL_DIAL` events shouldn't arrive.
    tester.uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;
    assert!(!tester.evdev().can_read()?);

    tester.evdev_mut().set_rel_mask(&rel_mask)?;
    Ok(())
}
