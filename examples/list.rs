//! Lists all input devices and their properties.

use std::{error::Error, fmt, io, process};

use evdevil::bits::{BitSet, BitValue};

fn main() {
    env_logger::init();
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\nerror: {e}");
            let mut error: &dyn Error = &e;
            while let Some(source) = error.source() {
                eprintln!("- caused by: {source}");
                error = source;
            }
            process::exit(1);
        }
    }
}

fn run() -> io::Result<()> {
    for res in evdevil::enumerate()?.with_path() {
        let (path, device) = res?;
        println!("- {}", path.display());
        println!("  id: {:?}", device.input_id()?);
        println!("  name: {:?}", device.name()?);
        println!("  location: {:?}", device.phys()?);
        println!("  unique id: {:?}", device.unique_id()?);
        println!("  props: {:?}", device.props()?);
        println!("  supported events: {:?}", device.supported_events()?);
        dump_codes("EV_KEY", device.supported_keys());
        dump_codes("EV_REL", device.supported_rel_axes());
        dump_codes("EV_ABS", device.supported_abs_axes());
        dump_codes("EV_MSC", device.supported_misc());
        dump_codes("EV_SW", device.supported_switches());
        dump_codes("EV_LED", device.supported_leds());
        dump_codes("EV_SND", device.supported_sounds());
        let abs_axes = device.supported_abs_axes()?;
        if !abs_axes.is_empty() {
            println!("  absolute axis ranges:");
            for abs in &abs_axes {
                let info = device.abs_info(abs)?;
                println!("  - {abs:?}: {info:?}");
            }
        }

        let ff = device.supported_ff_effects()?;
        if ff != 0 {
            println!(
                "  force-feedback support: {} x {:?}",
                ff,
                device.supported_ff_features()?
            );
        }
    }

    Ok(())
}

fn dump_codes<V>(name: &str, res: io::Result<BitSet<V>>)
where
    V: BitValue + fmt::Debug,
{
    match res {
        Ok(codes) => {
            if !codes.is_empty() {
                println!("  - {name}: {codes:?}");
            }
        }
        Err(e) => {
            println!("  - {name}: <{e}>");
        }
    }
}
