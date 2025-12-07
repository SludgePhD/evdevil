use std::{fmt, fs::File, io};

use crate::{event::InputEvent, write_raw};

/// Number of events to buffer before writing.
///
/// Picked semi-empirically based on how big event batches get for devices I own:
///
/// - Mouse: ~5 events when busy (2 axes + 2 buttons + 1 SYN_REPORT)
/// - Keyboard: ~10 events when a lot of keys are pressed at once.
/// - PS4 controller: ~8-9 events when a lot is going on (4 axes for analog sticks + 1-2 analog
///   triggers + 1-2 buttons + 1 SYN_REPORT).
/// - PS4 motion sensors: ~8 (3 acc. + 3 gyro + 1 timestamp + 1 SYN_REPORT)
/// - Laptop Touchpad: ~10 when using 2 fingers (3 for each MT slot position update + 2 ABS_{X,Y}
///   + 1 timestamp + 1 SYN_REPORT)
const BATCH_WRITE_SIZE: usize = 12;

pub(crate) struct BatchWriter {
    buffer: [InputEvent; BATCH_WRITE_SIZE],
    bufpos: usize,
}

impl fmt::Debug for BatchWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchWriter")
            .field("buffer", &&self.buffer[..self.bufpos])
            .finish()
    }
}

impl BatchWriter {
    pub(crate) fn new() -> Self {
        BatchWriter {
            buffer: [InputEvent::zeroed(); BATCH_WRITE_SIZE],
            bufpos: 0,
        }
    }

    pub(crate) fn write(&mut self, events: &[InputEvent], file: &File) -> io::Result<()> {
        self.write_to(events, |ev| write_raw(file, ev))
    }

    pub(crate) fn flush(&mut self, file: &File) -> io::Result<()> {
        self.flush_to(|ev| write_raw(file, ev))
    }

    fn write_to<W>(&mut self, events: &[InputEvent], mut writer: W) -> io::Result<()>
    where
        W: FnMut(&[InputEvent]) -> io::Result<()>,
    {
        let remaining = self.buffer.len() - self.bufpos;

        if events.len() > remaining {
            // Doesn't fit in the buffer, so empty the buffer.
            self.flush_to(&mut writer)?;
        }
        if events.len() >= BATCH_WRITE_SIZE {
            // Incoming events would completely fill the buffer, so flush and write them directly.
            self.flush_to(&mut writer)?;
            return writer(events);
        }

        // `events` fit in `self.buffer`.
        self.buffer[self.bufpos..][..events.len()].copy_from_slice(events);
        self.bufpos += events.len();

        Ok(())
    }

    fn flush_to<W>(&mut self, mut writer: W) -> io::Result<()>
    where
        W: FnMut(&[InputEvent]) -> io::Result<()>,
    {
        let is_empty = self.bufpos == 0;
        if !is_empty {
            writer(&self.buffer[..self.bufpos])?;
            self.bufpos = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_writer() -> io::Result<()> {
        let mut w = BatchWriter::new();
        w.write_to(&[InputEvent::zeroed(); BATCH_WRITE_SIZE - 1], |_| {
            unreachable!("shouldn't write them yet")
        })?;
        w.write_to(&[InputEvent::zeroed(); 1], |_| {
            unreachable!("shouldn't write them yet")
        })?;

        let mut wrote = Vec::new();
        w.write_to(&[InputEvent::zeroed()], |ev| {
            wrote.push(ev.len());
            Ok(())
        })?;
        assert_eq!(wrote, &[BATCH_WRITE_SIZE], "should have written events");
        assert_eq!(w.bufpos, 1, "should have 1 event in the buffer");

        // Doesn't fit in the buffer, so it will be written directly.
        let mut wrote = Vec::new();
        w.write_to(&[InputEvent::zeroed(); BATCH_WRITE_SIZE + 1], |ev| {
            wrote.push(ev.len());
            Ok(())
        })?;
        assert_eq!(wrote, &[1, BATCH_WRITE_SIZE + 1]);
        assert_eq!(w.bufpos, 0);

        // Equal to the buffer size, so it will be written directly.
        let mut wrote = Vec::new();
        w.write_to(&[InputEvent::zeroed(); BATCH_WRITE_SIZE], |ev| {
            wrote.push(ev.len());
            Ok(())
        })?;
        assert_eq!(wrote, &[BATCH_WRITE_SIZE]);
        assert_eq!(w.bufpos, 0);

        // If there's 1 event in the buffer, and we write a whole batch worth, flush the buffer,
        // then write the new events directly. Result is that the buffer is empty.
        w.write_to(&[InputEvent::zeroed(); 1], |_| {
            unreachable!("shouldn't write them yet")
        })?;
        assert_eq!(w.bufpos, 1);

        let mut wrote = Vec::new();
        w.write_to(&[InputEvent::zeroed(); BATCH_WRITE_SIZE], |ev| {
            wrote.push(ev.len());
            Ok(())
        })?;
        assert_eq!(wrote, &[1, BATCH_WRITE_SIZE]);
        assert_eq!(w.bufpos, 0);

        w.flush_to(|_| {
            unreachable!("should not flush anything");
        })?;

        Ok(())
    }
}
