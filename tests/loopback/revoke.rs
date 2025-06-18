use std::{error::Error, io};

use evdevil::{
    Evdev,
    event::{Rel, RelEvent},
};

use crate::Tester;

#[test]
fn revoke() -> io::Result<()> {
    let t = Tester::get();
    let dev = t.evdev();
    let dev2 = Evdev::open(t.evdev().path().unwrap())?;
    assert!(!dev2.can_read()?);

    // After revocation, dev2 shouldn't receive any events anymore
    dev2.revoke()?;
    t.uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;

    assert!(dev.can_read()?);
    assert!(!dev2.can_read()?);

    // Further uses of `dev2` (via `write` or `ioctl`) result in `ENODEV`.
    match dev2.revoke() {
        Err(e) => {
            let mut e: &dyn Error = &e;
            while let Some(s) = e.source() {
                e = s;
            }
            let os = e
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error()
                .unwrap();
            assert_eq!(os, libc::ENODEV);
        }
        e => panic!("unexpected result: {e:?}"),
    }

    while dev.can_read()? {
        dev.raw_events().next().unwrap()?;
    }

    Ok(())
}
