use std::io;

use evdevil::event::Abs;

use crate::Tester;

#[test]
fn smoke() -> io::Result<()> {
    let mut t = Tester::get();

    t.with_reader(|uinput, reader| {
        uinput
            .writer()
            .slot(0)?
            .set_tracking_id(5)?
            .set_position(123, -400)?
            .set_tracking_id(0)?
            .finish_slot()?
            .slot(3)?
            .set_tracking_id(7)?
            .set_position(900, 999)?
            .finish_slot()?
            .finish()?;

        reader.update()?;
        assert_eq!(reader.current_slot(), 3);

        let slots = reader.valid_slots().collect::<Vec<_>>();
        assert_eq!(slots, &[0, 3]);

        assert_eq!(reader.slot_state(0, Abs::MT_TRACKING_ID), Some(0));
        assert_eq!(reader.slot_state(0, Abs::MT_POSITION_X), Some(123));
        assert_eq!(reader.slot_state(0, Abs::MT_POSITION_Y), Some(-400));

        assert_eq!(reader.slot_state(3, Abs::MT_TRACKING_ID), Some(7));
        assert_eq!(reader.slot_state(3, Abs::MT_POSITION_X), Some(900));
        assert_eq!(reader.slot_state(3, Abs::MT_POSITION_Y), Some(999));

        uinput
            .writer()
            .slot(0)?
            .set_tracking_id(-1)?
            .finish_slot()?
            .finish()?;

        reader.update()?;

        let slots = reader.valid_slots().collect::<Vec<_>>();
        assert_eq!(slots, &[3]);

        Ok(())
    })?;

    Ok(())
}
