use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    mem::{self, MaybeUninit},
    slice,
    vec::Drain,
};

use crate::event::{EventKind, InputEvent, Syn};

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

/// 21 * 24 bytes = 504 bytes, so that we fill a 512 B allocation size class with little waste
/// (assuming one exists, etc.).
const BATCH_READ_SIZE: usize = 21;

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

fn write_raw(mut file: &File, events: &[InputEvent]) -> io::Result<()> {
    unsafe {
        let bytes = slice::from_raw_parts(
            events.as_ptr().cast::<u8>(),
            mem::size_of::<InputEvent>() * events.len(),
        );
        file.write_all(bytes)?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct BatchReader {
    /// We don't know beforehand how many events until the next SYN_REPORT, so we have to buffer
    /// them in this `Vec`.
    buf: Vec<InputEvent>,
}

pub(crate) type BatchDrain<'a> = Drain<'a, InputEvent>;

impl BatchReader {
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Returns `Ok(None)` when events were read, but there is no SYN event to finish a batch.
    /// Return `Ok(Some(drain))` when a batch has been completed by a SYN event. The `drain`
    /// iterator yields all events up to and including the SYN event.
    ///
    /// Note that this does not distinguish between the different SYN events – SYN_REPORT and
    /// SYN_DROPPED will both cause all preceding events to be yielded to the caller.
    pub(crate) fn fill(&mut self, file: &File) -> io::Result<Option<BatchDrain<'_>>> {
        self.buf.reserve(BATCH_READ_SIZE);

        // note that `dest` may be bigger than `BATCH_READ_SIZE`
        let len = self.buf.len();
        let dest = self.buf.spare_capacity_mut();
        let n = read_raw(file, dest)?;
        assert_ne!(n, 0);
        unsafe {
            self.buf.set_len(len + n);
        }

        // Find the index of the next SYN_REPORT or SYN_DROPPED event.
        // Unknown SYN_* events are kept in the buffer until committed by a SYN_REPORT or discarded
        // by a SYN_DROPPED.
        let new = &self.buf[len..];
        let pos = new
            .iter()
            .position(|ev| match ev.kind() {
                Some(EventKind::Syn(ev)) => ev.syn() == Syn::REPORT || ev.syn() == Syn::DROPPED,
                _ => false,
            })
            .map(|i| i + len);
        // FIXME: if we've read 2 or more reports, this will leave one or more SYN_REPORTs in the
        // buffer

        Ok(pos.map(|i| self.buf.drain(..=i)))
    }
}

fn read_raw(mut file: &File, dest: &mut [MaybeUninit<InputEvent>]) -> io::Result<usize> {
    let bptr = dest.as_mut_ptr().cast::<u8>();
    let byte_buf =
        unsafe { slice::from_raw_parts_mut(bptr, mem::size_of::<InputEvent>() * dest.len()) };
    let bytes = file.read(byte_buf)?;
    debug_assert_eq!(bytes % mem::size_of::<InputEvent>(), 0);
    Ok(bytes / mem::size_of::<InputEvent>())
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
