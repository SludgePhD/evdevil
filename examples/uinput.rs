//! Creates a uinput device and presses a key repeatedly.

use std::{io, thread, time::Duration};

use evdevil::{
    event::{EventKind, Key, KeyEvent, KeyState, UinputCode},
    ff::Feature,
    uinput::UinputDevice,
};

/// The key or button to press and release.
///
/// Set this to `Key::KEY_A` to make the effect visible in applications.
const KEY: Key = Key::BTN_TRIGGER_HAPPY1;

fn main() -> io::Result<()> {
    env_logger::init();
    let dev = UinputDevice::builder()?
        .with_keys([KEY])?
        .with_ff_features([Feature::RUMBLE, Feature::GAIN])?
        .with_ff_effects_max(10)?
        .build("Rust UInput")?;

    println!("Created device");

    // Use a separate thread to submit button presses to the device:
    let dev2 = dev.try_clone()?;
    thread::spawn(move || {
        loop {
            dev2.write(&[KeyEvent::new(KEY, KeyState::PRESSED).into()])
                .unwrap();
            println!("Key pressed");
            thread::sleep(Duration::from_millis(500));

            dev2.write(&[KeyEvent::new(KEY, KeyState::RELEASED).into()])
                .unwrap();
            println!("Key released");
            thread::sleep(Duration::from_millis(500));
        }
    });

    for res in dev.events() {
        let event = res?;
        println!("Received event: {event:?}");
        match event.kind() {
            EventKind::Uinput(ev) => match ev.code() {
                UinputCode::FF_UPLOAD => dev.ff_upload(&ev, |upl| {
                    println!("Force-Feedback upload: {upl:?}");
                    Ok(())
                })?,
                UinputCode::FF_ERASE => dev.ff_erase(&ev, |erase| {
                    println!("Force-Feedback erase: {erase:?}");
                    Ok(())
                })?,
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}
