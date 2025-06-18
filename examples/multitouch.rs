use std::{env, io, process, thread, time::Duration};

use evdevil::{
    Evdev, Slot,
    event::{Abs, MtToolType},
};

fn main() -> io::Result<()> {
    env_logger::init();
    let evdev = match &*env::args().skip(1).collect::<Vec<_>>() {
        [evdev] => Evdev::open(evdev)?,
        _ => {
            eprintln!("usage: {} <evdev-path>", env!("CARGO_CRATE_NAME"));
            process::exit(1);
        }
    };

    let mut reader = evdev.into_reader()?;
    println!("Opened {}", reader.evdev().name()?);
    println!("ABS axes: {:?}", reader.evdev().supported_abs_axes()?);
    let mut points = Vec::new();
    loop {
        reader.update()?;

        let mut new_points = Vec::new();
        for slot in reader.valid_slots() {
            new_points.push(Touch {
                slot,
                id: reader.slot_state(slot, Abs::MT_TRACKING_ID),
                tool: reader
                    .slot_state(slot, Abs::MT_TOOL_TYPE)
                    .map(MtToolType::from_raw),
                x: reader.slot_state(slot, Abs::MT_POSITION_X),
                y: reader.slot_state(slot, Abs::MT_POSITION_Y),
                pressure: reader.slot_state(slot, Abs::MT_PRESSURE),
                distance: reader.slot_state(slot, Abs::MT_DISTANCE),
            });
        }

        if points != new_points {
            points = new_points;
            println!("------------------");
            for point in &points {
                println!("{point:?}");
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Touch {
    slot: Slot,
    id: Option<i32>,
    tool: Option<MtToolType>,
    x: Option<i32>,
    y: Option<i32>,
    pressure: Option<i32>,
    distance: Option<i32>,
}
