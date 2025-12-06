//! A simple rumble effect demo.

use std::{
    env,
    io::{self, stdin},
    process, thread,
};

use evdevil::{
    Evdev,
    event::{EventType, ForceFeedbackEvent, Key},
    ff::{Effect, Feature, Rumble, Trigger},
};

macro_rules! bail {
    ($($args:tt)*) => {
        return Err(io::Error::other(format!($($args)*)))
    };
}

fn main() -> io::Result<()> {
    env_logger::init();
    let evdev = match &*env::args().skip(1).collect::<Vec<_>>() {
        [evdev] => Evdev::open(evdev)?,
        _ => {
            eprintln!("usage: {} <evdev-path>", env!("CARGO_CRATE_NAME"));
            process::exit(1);
        }
    };

    let name = evdev.name()?;
    println!("Opened {name}");
    let max_effects = evdev.supported_ff_effects()?;
    if max_effects == 0 {
        bail!("Device '{}' does not support force-feedback", name);
    }

    let feat = evdev.supported_ff_features()?;
    println!("Supported force-feedback effects: {feat:?}");

    if !feat.contains(Feature::RUMBLE) {
        bail!("Rumble effects are not supported");
    }

    let dev2 = evdev.try_clone()?;
    thread::spawn(move || {
        for res in dev2.raw_events() {
            let should_print = match res {
                Ok(ev) => match ev.event_type() {
                    EventType::ABS | EventType::REL | EventType::KEY | EventType::SYN => false,
                    _ => true,
                },
                Err(_) => true,
            };
            if should_print {
                println!("<- {res:?}");
            }
            if res.is_err() {
                return;
            }
        }
    });

    println!("Uploading effect");
    let id = evdev.upload_ff_effect(
        Effect::from(Rumble::new(30000, 30000)).with_trigger(Trigger::new(Key::BTN_SOUTH, 0)),
    )?;

    println!("Press enter to start/stop the effect");

    let mut active = false;
    for line in stdin().lines() {
        let _line = line?;
        active = !active;
        evdev.write(&[ForceFeedbackEvent::new_control_effect(id, active).into()])?;
    }

    Ok(())
}
