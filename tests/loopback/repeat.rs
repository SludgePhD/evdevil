use std::io;

use evdevil::{
    KeyRepeat,
    event::{EventKind, Rel, RelEvent, Syn},
};

use crate::{KEY_REPEAT, Tester};

#[test]
#[cfg_attr(target_os = "freebsd", ignore = "events do not echo back on FreeBSD")]
fn get_set_repeat() -> io::Result<()> {
    let t = Tester::get();

    let rep = t.evdev().key_repeat()?;
    assert_eq!(rep, Some(KEY_REPEAT));
    assert!(!t.evdev().is_readable()?);

    // Use uinput to change the repeat settings
    t.uinput
        .writer()
        .set_key_repeat(KeyRepeat::new(100, 20))?
        .finish()?;

    assert!(t.uinput.is_readable()?);
    match t.uinput.events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }
    match t.uinput.events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }

    assert!(t.evdev().is_readable()?);
    match t.evdev().raw_events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }
    match t.evdev().raw_events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }
    match t.evdev().raw_events().next().unwrap()?.kind() {
        EventKind::Syn(ev) if ev.syn() == Syn::REPORT => {}
        e => panic!("unexpected event {e:?}"),
    }
    assert!(!t.evdev().is_readable()?);

    // Use evdev to change the repeat settings back
    t.evdev().set_key_repeat(KEY_REPEAT)?;
    assert!(t.uinput.is_readable()?);
    match t.uinput.events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }
    match t.uinput.events().next().unwrap()?.kind() {
        EventKind::Repeat(_) => {}
        e => panic!("unexpected event {e:?}"),
    }

    // Similar to the bug around force-feedback events, the kernel will insert additional copies of
    // the `REP_*` events when subsequent tests use `uinput.write`.
    // So we write a `Rel` event and drain the evdev buffer to avoid that.
    t.uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;
    assert!(t.evdev().is_readable()?);
    while t.evdev().is_readable()? {
        t.evdev().raw_events().next().unwrap()?;
    }

    Ok(())
}
