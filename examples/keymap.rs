use std::{env, io, process, str::FromStr};

use evdevil::{Evdev, Scancode, event::Key};

fn main() -> io::Result<()> {
    match &*env::args().skip(1).collect::<Vec<_>>() {
        [path] => dump_keymap(&Evdev::open(path)?),
        [path, arg] if arg.contains('=') => {
            let Some((scancode, key)) = arg.split_once('=') else {
                unreachable!()
            };

            let key = Key::from_str(key)?;
            let scancode = u32::from_str_radix(scancode, 16)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let scancode = Scancode::from(scancode);

            println!("setting scancode {scancode} -> {key:?}");
            Evdev::open(path)?.set_keymap_entry(scancode, key)?;

            Ok(())
        }
        [path, scancode] => {
            let scancode = u32::from_str_radix(scancode, 16)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let scancode = Scancode::from(scancode);
            query_code(&Evdev::open(path)?, scancode)
        }
        _ => {
            eprintln!("usage: keymap <path> [scancode[=key]]");
            eprintln!();
            eprintln!("If [scancode=key] is absent, prints the whole keymap.");
            eprintln!("If only a scancode is provided, prints the key associated with it.");
            eprintln!(
                "`scancode` must be a hexadecimal code, `key` must be the \
                name of one of the evdev keycodes (eg. `KEY_BACKSPACE`)"
            );
            process::exit(1);
        }
    }
}

fn dump_keymap(device: &Evdev) -> io::Result<()> {
    let keys = device.supported_keys()?;
    if !keys.is_empty() && device.keymap_entry_by_index(0).is_ok() {
        println!("  keymap:");
        for i in 0.. {
            let Some(ent) = device.keymap_entry_by_index(i)? else {
                break;
            };

            println!("  - {:?}", ent);
        }
    } else {
        println!("this device does not have a keymap");
    }
    Ok(())
}

fn query_code(device: &Evdev, code: Scancode) -> io::Result<()> {
    let ent = device.keymap_entry(code)?;
    match ent {
        Some(ent) => println!("scancode {code} is mapped to {:?}", ent.keycode()),
        None => println!("scancode {code} not found in keymap"),
    }
    Ok(())
}
