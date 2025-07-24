//! Listens for hotplug events.

use std::io;

use evdevil::hotplug::HotplugMonitor;

fn main() -> io::Result<()> {
    env_logger::init();
    let mon = HotplugMonitor::new()?;
    for res in mon {
        let ev = res?.open()?;
        println!("{}", ev.name()?);
    }
    Ok(())
}
