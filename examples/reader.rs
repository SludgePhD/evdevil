use std::{env, io, process};

use evdevil::Evdev;

fn main() -> io::Result<()> {
    env_logger::init();
    let evdev = match &*env::args().skip(1).collect::<Vec<_>>() {
        [evdev] => Evdev::open(evdev)?,
        _ => {
            eprintln!("usage: {} <evdev-path>", env!("CARGO_CRATE_NAME"));
            process::exit(1);
        }
    };

    for event in evdev.into_reader()? {
        let event = event?;
        println!("{event:?}");
    }
    Ok(())
}
