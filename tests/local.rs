use std::io;

#[test]
fn enumerate_local_devices() -> io::Result<()> {
    for res in evdevil::enumerate()? {
        let evdev = res?;
        let mut reader = evdev.into_reader()?;
        reader.update()?;
    }

    Ok(())
}
