//! Tests against the local system.

use core::{any::type_name, str::FromStr};
use std::{collections::HashMap, fs, io};

use evdevil::event::{Abs, Key, Led, Misc, Rel, Sound, Switch};

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

/// Parses the system's local `input-event-codes.h` and compares our event code values against it.
///
/// Depending on what system runs the test, the header file might be missing some newer constants,
/// but all constants that are present should match `evdevil`'s own.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "only works on Linux")]
fn event_codes() -> io::Result<()> {
    static PATH: &str = "/usr/include/linux/input-event-codes.h";
    static SKIP: &[&str] = &["KEY_MIN_INTERESTING"];

    let contents = match fs::read_to_string(PATH) {
        Ok(contents) => contents,
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("failed to read '{PATH}': {e}"),
            ));
        }
    };

    let mut defines = HashMap::new();
    for line in contents.lines() {
        let Some(line) = line.strip_prefix("#define") else {
            continue;
        };

        let mut split = line.split_ascii_whitespace();
        let name = split.next().unwrap();
        let Some(value) = split.next() else {
            continue;
        };

        if SKIP.contains(&name) {
            continue;
        }

        if name.ends_with("_CNT") || name.ends_with("_MAX") {
            continue;
        }

        let value = match defines.get(value) {
            Some(val) => *val,
            None => {
                let n = if value.starts_with("0x") {
                    u32::from_str_radix(&value[2..], 16)
                } else {
                    u32::from_str(value)
                };
                match n {
                    Ok(n) => n,
                    Err(e) => {
                        panic!("failed to parse value '{value}' of constant '{name}': {e}");
                    }
                }
            }
        };
        defines.insert(name, value);

        // FIXME: we skip `InputProp`, `EventType`, `Syn` and `Bus` because they aren't parseable
        // (not sure if `FromStr` would be useful there)
        if name.starts_with("KEY_") || name.starts_with("BTN_") {
            check::<Key>(name, value);
        } else if name.starts_with("REL_") {
            check::<Rel>(name, value);
        } else if name.starts_with("ABS_") {
            check::<Abs>(name, value);
        } else if name.starts_with("SW_") {
            check::<Switch>(name, value);
        } else if name.starts_with("MSC_") {
            check::<Misc>(name, value);
        } else if name.starts_with("LED_") {
            check::<Led>(name, value);
        } else if name.starts_with("SND_") && !name.starts_with("SND_PROFILE") {
            check::<Sound>(name, value);
        } else {
            continue;
        }
    }

    Ok(())
}

trait Raw {
    fn raw(self) -> u32;
}
impl Raw for Key {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Rel {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Abs {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Switch {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Misc {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Led {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
impl Raw for Sound {
    fn raw(self) -> u32 {
        self.raw().into()
    }
}
fn check<T>(s: &str, expected_value: u32)
where
    T: FromStr + Raw,
{
    match T::from_str(s) {
        Ok(t) => {
            let raw = t.raw();
            assert_eq!(
                raw, expected_value,
                "'{s}' parses into raw value {raw}, but the header specifies {expected_value}",
            );
        }
        Err(_) => {
            // This is not a fatal error because it most often indicates that a newly added constant
            // hasn't been added to `evdevil` yet.
            eprintln!("failed to parse '{s}' as a {}", type_name::<T>());
        }
    }
}
