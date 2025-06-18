use std::io;

use evdevil::event::{EventKind, Rel, RelEvent, Syn};

use crate::Tester;

#[test]
fn set_clockid() -> io::Result<()> {
    let t = Tester::get();

    t.uinput.write(&[RelEvent::new(Rel::DIAL, 78).into()])?;
    let ev = t.evdev().raw_events().next().unwrap()?;
    match ev.kind() {
        Some(EventKind::Rel(ev)) if ev.rel() == Rel::DIAL && ev.value() == 78 => {}
        _ => panic!("unexpected event: {ev:?}"),
    }
    let ev = t.evdev().raw_events().next().unwrap()?;
    match ev.kind() {
        Some(EventKind::Syn(ev)) if ev.syn() == Syn::REPORT => {}
        _ => panic!("unexpected event: {ev:?}"),
    }

    let wall_time = ev.time();

    t.evdev().set_clockid(libc::CLOCK_MONOTONIC)?;

    t.uinput.write(&[RelEvent::new(Rel::DIAL, 78).into()])?;
    let ev = t.evdev().raw_events().next().unwrap()?;
    match ev.kind() {
        Some(EventKind::Rel(rel)) if rel.rel() == Rel::DIAL && rel.value() == 78 => {}
        _ => panic!("unexpected event: {ev:?}"),
    }
    let ev = t.evdev().raw_events().next().unwrap()?;
    match ev.kind() {
        Some(EventKind::Syn(ev)) if ev.syn() == Syn::REPORT => {}
        _ => panic!("unexpected event: {ev:?}"),
    }

    // Monotonic time typically starts at 0 at boot, while wall time's zero is at 1-1-1970.
    // So even though the event with monotonic clock was emitted later, it should have an earlier
    // timestamp.
    let monotonic_time = ev.time();
    assert!(
        monotonic_time < wall_time,
        "{monotonic_time:?} <-> {wall_time:?}"
    );

    // Restore default:
    t.evdev().set_clockid(libc::CLOCK_REALTIME)?;

    Ok(())
}
