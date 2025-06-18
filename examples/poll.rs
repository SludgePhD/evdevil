//! Continuously polls the key/switch/LED state and prints it.
//!
//! Uses the state polling interface for fetching the device state, not the event stream interface.

use std::{env, io, process, thread, time::Duration};

use evdevil::{Evdev, bits::BitSet};

fn main() -> io::Result<()> {
    env_logger::init();
    let evdev = match &*env::args().skip(1).collect::<Vec<_>>() {
        [evdev] => Evdev::open(evdev)?,
        _ => {
            eprintln!("usage: {} <evdev-path>", env!("CARGO_CRATE_NAME"));
            process::exit(1);
        }
    };

    let mut keys = BitSet::new();
    let mut leds = BitSet::new();
    let mut sounds = BitSet::new();
    let mut switches = BitSet::new();

    loop {
        let new_keys = evdev.key_state()?;
        let new_leds = evdev.led_state()?;
        let new_sounds = evdev.sound_state()?;
        let new_switches = evdev.switch_state()?;

        if keys != new_keys || leds != new_leds || sounds != new_sounds || switches != new_switches
        {
            keys = new_keys;
            leds = new_leds;
            sounds = new_sounds;
            switches = new_switches;
            println!("-------------------------------");
            println!("keys: {keys:?}");
            println!("leds: {leds:?}");
            println!("sounds: {sounds:?}");
            println!("switches: {switches:?}");
        }

        thread::sleep(Duration::from_millis(50));
    }
}
