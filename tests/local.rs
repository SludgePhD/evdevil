use std::io;

/// Tests that all devices on the local system can be enumerated, and that `EventReader`
/// successfully synchronizes.
#[test]
fn enumerate_local_devices() -> io::Result<()> {
    for res in evdevil::enumerate()? {
        let (_, evdev) = res?;
        let mut reader = evdev.into_reader()?;
        reader.update()?;
    }

    Ok(())
}
