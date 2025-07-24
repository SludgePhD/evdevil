use std::{io, thread, time::Duration};

use evdevil::{hotplug::HotplugMonitor, uinput::UinputDevice};

const DEVICE_NAME: &str = "-@-rust-hotplug-test-@-";

fn main() -> io::Result<()> {
    env_logger::init();

    let mon = match HotplugMonitor::new() {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            eprintln!("hotplug is not supported on this platform; skipping test");
            return Ok(());
        }
        res => res?,
    };

    // This is just for testing non-blocking mode.
    mon.set_nonblocking(true)?;

    // Creating the device like this should cause the event to fire.
    println!("creating `uinput` device");
    let _dev = UinputDevice::builder()?.build(DEVICE_NAME)?;

    println!("waiting for hotplug event...");
    loop {
        thread::sleep(Duration::from_millis(25));

        for res in &mon {
            let dev = res?.open()?;
            let name = dev.name()?;
            if name == DEVICE_NAME {
                println!("success! found test device at {}", dev.path().display());
                return Ok(());
            } else {
                println!("found non-matching device '{name}'");
            }
        }

        println!("no results so far, blocking...");
        mon.set_nonblocking(false)?;
    }
}
