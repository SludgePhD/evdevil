use std::{env, io, process, thread, time::Duration};

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

    println!("Grabbing '{}' for 3 seconds", evdev.name()?);

    evdev.grab()?;
    thread::sleep(Duration::from_secs(3));

    println!("Ungrabbing device");

    evdev.ungrab()?;

    println!("Done!");
    Ok(())
}
