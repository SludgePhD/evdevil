//! A convenient API for robustly reading device events.

mod r#async;

#[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async-io"))))]
#[cfg(any(feature = "tokio", feature = "async-io"))]
pub use r#async::{AsyncEvents, AsyncReports};

#[cfg(test)]
mod tests;

use std::{
    collections::VecDeque,
    fmt,
    fs::File,
    io::{self, Read},
    iter::{self, FusedIterator, zip},
    mem,
    ops::RangeInclusive,
    os::fd::{AsFd, AsRawFd, BorrowedFd, IntoRawFd, RawFd},
    slice,
    sync::Arc,
    time::{Instant, SystemTime},
};

use crate::{
    Evdev, Slot,
    bits::{BitSet, BitValue},
    drop::on_drop,
    event::{
        Abs, AbsEvent, EventKind, EventType, InputEvent, Key, KeyEvent, KeyState, Led, LedEvent,
        Sound, SoundEvent, Switch, SwitchEvent, Syn, SynEvent,
    },
    raw::input::EVIOCGMTSLOTS,
};

const MAX_MT_SLOTS: i32 = 60;

/// Storage for the current multitouch state.
#[derive(Clone, PartialEq)]
struct MtStorage {
    /// The data buffer contains `codes` number of groups, each prefixed by the `ABS_MT_*` axis
    /// code followed by `slots` values of that code.
    data: Vec<i32>,
    /// Number of MT slots supported by the device (`maximum` value of the `ABS_MT_SLOT` axis).
    slots: u32,
    /// Number of supported `ABS_*` codes between `ABS_MT_SLOT+1` and `ABS_MAX`.
    codes: u32,
    /// Selected MT slot (current value of the `ABS_MT_SLOT` axis).
    active_slot: u32,
}

impl fmt::Debug for MtStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct FmtData<'a> {
            data: &'a [i32],
            slots: usize,
        }

        impl fmt::Debug for FmtData<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut list = f.debug_list();
                for chunk in self.data.chunks(self.slots + 1) {
                    list.entry(&Abs::from_raw(chunk[0] as u16));
                    list.entries(&chunk[1..]);
                }
                list.finish()
            }
        }

        f.debug_struct("MtStorage")
            .field("slots", &self.slots)
            .field("codes", &self.codes)
            .field("active_slot", &self.active_slot)
            .field(
                "data",
                &FmtData {
                    data: &self.data,
                    slots: self.slots as usize,
                },
            )
            .finish()
    }
}

impl MtStorage {
    fn empty() -> Self {
        Self {
            data: Vec::new(),
            slots: 0,
            codes: 0,
            active_slot: 0,
        }
    }

    fn current(evdev: &Evdev, abs_axes: &BitSet<Abs>) -> io::Result<Self> {
        let mut this = Self {
            data: Vec::new(),
            slots: 0,
            codes: 0,
            active_slot: 0,
        };

        if !abs_axes.contains(Abs::MT_SLOT) {
            return Ok(this);
        }
        if !abs_axes.contains(Abs::MT_TRACKING_ID) {
            log::warn!(
                "device {} advertises support for `ABS_MT_SLOT` but not `ABS_MT_TRACKING_ID`; multitouch support will not work",
                evdev.name().unwrap_or_else(|e| e.to_string()),
            );
            return Ok(this);
        }

        let mt_slot_info = evdev.abs_info(Abs::MT_SLOT)?;
        if mt_slot_info.minimum() != 0 {
            log::warn!("`ABS_MT_SLOT` has a non-0 minimum: {:?}", mt_slot_info);
        }

        let slot_count = mt_slot_info.maximum().saturating_add(1);
        if mt_slot_info.maximum() > MAX_MT_SLOTS {
            log::warn!(
                "`ABS_MT_SLOT` declares too many slots: {:?} (only the first {} will be used)",
                mt_slot_info,
                MAX_MT_SLOTS,
            );
        }
        this.slots = slot_count.clamp(0, MAX_MT_SLOTS) as u32;
        this.active_slot = mt_slot_info.value().max(0) as u32;
        this.data.clear();
        this.codes = 0;

        for mt_code in Abs::MT_SLOT.raw() + 1..Abs::MAX.raw() {
            if !abs_axes.contains(Abs::from_raw(mt_code)) {
                continue;
            }

            // `mt_code` is supported; fetch its current value for all slots, appending it to `data`
            this.codes += 1;
            let start_idx = this.data.len();
            this.data
                .resize(this.data.len() + 1 + this.slots as usize, 0);
            this.data[start_idx] = mt_code.into();

            unsafe {
                evdev.ioctl(
                    "EVIOCGMTSLOTS",
                    EVIOCGMTSLOTS((this.slots as usize + 1) * 4),
                    this.data[start_idx..].as_mut_ptr().cast(),
                )?;
            }
        }
        this.data.shrink_to_fit();

        Ok(this)
    }

    fn resync_from(
        &mut self,
        src: &MtStorage,
        queue: &mut VecDeque<InputEvent>,
        last_event: SystemTime,
    ) {
        let mut emit = |ev: InputEvent| {
            queue.push_back(ev.with_time(last_event));
        };

        // `self` can be empty and `src` may be populated here.
        assert!(
            self.data.is_empty() || self.data.len() == src.data.len(),
            "`self` must either be empty or have the same layout as `src`",
        );

        let was_empty = self.data.is_empty();
        self.slots = src.slots;
        self.codes = src.codes;
        self.data.resize(src.data.len(), 0);

        let chunk_size = self.slots as usize + 1;
        for slot in 0..self.slots {
            let mut slot_active = false;
            for code in 0..self.codes {
                let dest_chunk = &mut self.data[code as usize * chunk_size..][..chunk_size];
                let src_chunk = &src.data[code as usize * chunk_size..][..chunk_size];
                if dest_chunk[0] == 0 {
                    dest_chunk[0] = src_chunk[0];
                } else {
                    assert_eq!(dest_chunk[0], src_chunk[0]);
                }

                let abs = Abs::from_raw(src_chunk[0] as _);

                let dest = &mut dest_chunk[slot as usize + 1];
                let src = &src_chunk[slot as usize + 1];
                if *dest != *src {
                    // For `ABS_MT_TRACKING_ID`, the "default state" should be -1 instead of 0 like
                    // it is for other axes. This avoids emitting useless events when an
                    // `EventReader` is first created.
                    if abs == Abs::MT_TRACKING_ID && *src == -1 && was_empty {
                        *dest = *src;
                        continue;
                    }

                    *dest = *src;
                    if !slot_active {
                        slot_active = true;
                        emit(AbsEvent::new(Abs::MT_SLOT, slot as _).into());
                    }

                    emit(AbsEvent::new(abs, *src as _).into());
                }
            }
        }

        if self.active_slot != src.active_slot {
            self.active_slot = src.active_slot;
            emit(AbsEvent::new(Abs::MT_SLOT, src.active_slot as _).into());
        }
    }

    /// Iterator over code groups; each slice has `slots + 1` entries, the first one being the
    /// `ABS_MT_*` code of the group.
    fn groups(&self) -> impl Iterator<Item = &[i32]> + '_ {
        self.data
            .chunks((self.slots + 1) as usize)
            .take(self.codes as usize)
    }
    fn groups_mut(&mut self) -> impl Iterator<Item = &mut [i32]> + '_ {
        self.data
            .chunks_mut((self.slots + 1) as usize)
            .take(self.codes as usize)
    }

    /// Returns a slice with 1 value of `code` per slot.
    ///
    /// `code` must be one of the `ABS_MT_*` codes (but not `ABS_MT_SLOT`).
    fn group_for_code(&self, code: Abs) -> Option<&[i32]> {
        if code.raw() <= Abs::MT_SLOT.raw() || code.raw() > Abs::MAX.raw() {
            return None;
        }
        self.groups().find_map(|grp| {
            if grp[0] == i32::from(code.raw()) {
                Some(&grp[1..])
            } else {
                None
            }
        })
    }

    fn mut_group_for_code(&mut self, code: Abs) -> Option<&mut [i32]> {
        if code.raw() <= Abs::MT_SLOT.raw() || code.raw() > Abs::MAX.raw() {
            return None;
        }
        self.groups_mut().find_map(|grp| {
            if grp[0] == i32::from(code.raw()) {
                Some(&mut grp[1..])
            } else {
                None
            }
        })
    }

    /// Iterator over all slot indices with valid data in them.
    fn valid_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.group_for_code(Abs::MT_TRACKING_ID)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter_map(|(slot, id)| {
                if *id == -1 {
                    None
                } else {
                    Some(Slot::from(slot as u16))
                }
            })
    }
}

#[derive(Debug)]
struct DeviceState {
    keys: BitSet<Key>,
    leds: BitSet<Led>,
    sounds: BitSet<Sound>,
    switches: BitSet<Switch>,
    abs: [i32; Abs::MT_SLOT.raw() as usize],
    abs_axes: BitSet<Abs>, // supported axes
    mt_storage: MtStorage,
    last_event: SystemTime,
}

impl DeviceState {
    /// Creates an empty device state, with no buttons pressed and all state at 0.
    fn new(abs_axes: BitSet<Abs>) -> Self {
        Self {
            keys: BitSet::new(),
            leds: BitSet::new(),
            sounds: BitSet::new(),
            switches: BitSet::new(),
            abs: [0; Abs::MT_SLOT.raw() as usize],
            abs_axes,
            mt_storage: MtStorage::empty(),
            // We emit events to update to the current device state, but without having any device
            // events available to get a timestamp from.
            // Default to `now()` so that there's a reasonable default time.
            // This should be the correct default time source, too.
            last_event: SystemTime::now(),
        }
    }

    /// Fetches the current state of the given device.
    fn current(evdev: &Evdev) -> io::Result<Self> {
        let abs_axes = evdev.supported_abs_axes()?;
        let mut abs = [0; Abs::MT_SLOT.raw() as usize];
        for (i, value) in abs.iter_mut().enumerate() {
            let axis = Abs::from_raw(i as _);
            if abs_axes.contains(axis) {
                let info = evdev.abs_info(axis)?;
                *value = info.value();
            }
        }

        Ok(Self {
            keys: evdev.key_state()?,
            leds: evdev.led_state()?,
            sounds: evdev.sound_state()?,
            switches: evdev.switch_state()?,
            abs,
            abs_axes,
            mt_storage: MtStorage::current(evdev, &abs_axes)?,
            last_event: SystemTime::now(),
        })
    }

    fn resync_from(&mut self, src: &DeviceState, queue: &mut VecDeque<InputEvent>) {
        fn sync_bitset<V: BitValue>(
            dest: &mut BitSet<V>,
            src: BitSet<V>,
            mut cb: impl FnMut(V, /* became set */ bool),
        ) {
            for value in dest.symmetric_difference(&src) {
                cb(value, src.contains(value));
            }

            *dest = src;
        }

        queue.clear();

        let mut emit = |ev: InputEvent| {
            queue.push_back(ev.with_time(self.last_event));
        };

        sync_bitset(&mut self.keys, src.keys, |key, on| {
            let state = if on {
                KeyState::PRESSED
            } else {
                KeyState::RELEASED
            };
            emit(KeyEvent::new(key, state).into());
        });
        sync_bitset(&mut self.leds, src.leds, |led, on| {
            emit(LedEvent::new(led, on).into());
        });
        sync_bitset(&mut self.sounds, src.sounds, |snd, playing| {
            emit(SoundEvent::new(snd, playing).into());
        });
        sync_bitset(&mut self.switches, src.switches, |sw, on| {
            emit(SwitchEvent::new(sw, on).into());
        });

        // Re-fetch values of all non-MT absolute axes
        for (abs, (dest, src)) in zip(&mut self.abs, src.abs).enumerate() {
            if *dest != src {
                emit(AbsEvent::new(Abs::from_raw(abs as u16), src).into());
                *dest = src;
            }
        }

        if self.abs_axes.contains(Abs::MT_SLOT) {
            // Re-fetch the state of every MT slot
            self.mt_storage
                .resync_from(&src.mt_storage, queue, self.last_event);
        }

        // If we emitted any synthetic events, follow up with a SYN_REPORT.
        // It's not clear if this is *strictly* necessary after a SYN_DROPPED: the kernel seems to
        // emit an empty report consisting of just a SYN_REPORT event after a SYN_DROPPED.
        // It is useful after the `EventReader` is just constructed though, since the event would
        // otherwise be missing.
        if !queue.is_empty() {
            log::debug!(
                "resync injected {} events -> adding SYN_REPORT",
                queue.len()
            );
            assert_ne!(queue.back().unwrap().event_type(), EventType::SYN);
            queue.push_back(SynEvent::new(Syn::REPORT).with_time(self.last_event));
        }
    }

    /// Fetches the current device state, and injects synthetic events to compensate for any
    /// difference to the expected state.
    ///
    /// # Postconditions
    ///
    /// - `queue` will either be empty, or its last element will be a SYN_REPORT.
    fn resync(&mut self, evdev: &Evdev, queue: &mut VecDeque<InputEvent>) -> io::Result<()> {
        let now = Instant::now();
        let _d = on_drop(|| log::debug!("`EventReader::resync` took {:?}", now.elapsed()));

        // Clear out all events, and drain the kernel buffer too, like libevdev does.
        let mut reads = 0;

        const READ_LIMIT: usize = 16;
        const READ_SIZE: usize = 128;
        while evdev.is_readable()? && reads < READ_LIMIT {
            let mut out = [InputEvent::zeroed(); READ_SIZE];
            read_raw(&evdev.file, &mut out)?;
            reads += 1;
        }
        if reads >= READ_LIMIT {
            log::warn!("resync: kernel buffer not empty after {reads}x{READ_SIZE} reads");
        }

        self.resync_from(&DeviceState::current(evdev)?, queue);
        Ok(())
    }

    /// Ingests an [`InputEvent`] and updates the local device state accordingly.
    fn update_state(&mut self, ev: InputEvent) {
        match ev.kind() {
            EventKind::Abs(ev) => {
                if ev.abs().raw() < Abs::MT_SLOT.raw() {
                    self.abs[ev.abs().raw() as usize] = ev.value();
                } else if ev.abs() == Abs::MT_SLOT {
                    self.mt_storage.active_slot = ev.value() as u32;
                } else {
                    let slot = self.mt_storage.active_slot;
                    if let Some(group) = self.mt_storage.mut_group_for_code(ev.abs()) {
                        if let Some(slot) = group.get_mut(slot as usize) {
                            *slot = ev.value();
                        }
                    }
                }
            }
            EventKind::Key(ev) => match ev.state() {
                KeyState::PRESSED => {
                    self.keys.insert(ev.key());
                }
                KeyState::RELEASED => {
                    self.keys.remove(ev.key());
                }
                _ => {}
            },
            EventKind::Led(ev) => {
                if ev.is_on() {
                    self.leds.insert(ev.led());
                } else {
                    self.leds.remove(ev.led());
                }
            }
            EventKind::Switch(ev) => {
                if ev.is_pressed() {
                    self.switches.insert(ev.switch());
                } else {
                    self.switches.remove(ev.switch());
                }
            }
            EventKind::Sound(ev) => {
                if ev.is_playing() {
                    self.sounds.insert(ev.sound());
                } else {
                    self.sounds.remove(ev.sound());
                }
            }
            _ => {}
        }
    }
}

/// Sans-I/O portion of the [`EventReader`] implementation.
#[derive(Debug)]
struct Impl {
    state: DeviceState,
    /// Queue of incoming events.
    ///
    /// Events are `read(2)` from the device into this queue, and are processed (updating the state
    /// of the `EventReader`) when they are pulled out of the queue by the [`Events`] or [`Reports`]
    /// iterators.
    ///
    /// Wrapped in an `Arc` to allow multiple `Report`s to coexist, if the caller insists on doing
    /// that (a lending iterator would be a better interface, but Rust doesn't have that yet).
    /// When reading more events into the queue, we use `make_mut` to obtain a `&mut`.
    incoming: Arc<VecDeque<InputEvent>>,
    /// Number of events to discard from the front of the queue before yielding the next report or
    /// event.
    skip: usize,
    /// Whether we need to discard (instead of queuing) all events until the next `SYN_REPORT`.
    ///
    /// Set after we get a `SYN_DROPPED` to clear out incomplete reports.
    discard_events: bool,
}

impl Impl {
    fn new(abs_axes: BitSet<Abs>) -> Self {
        Self {
            state: DeviceState::new(abs_axes),
            incoming: Arc::default(),
            skip: 0,
            discard_events: false,
        }
    }

    fn abs_state(&self, abs: Abs) -> i32 {
        self.state.abs[abs.raw() as usize]
    }

    fn valid_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.state.mt_storage.valid_slots()
    }

    fn slot_state(&self, slot: impl TryInto<Slot>, code: Abs) -> Option<i32> {
        assert!(
            code.raw() > Abs::MT_SLOT.raw(),
            "`slot_state` requires an `ABS_MT_*` value above `ABS_MT_SLOT`"
        );

        let slot: Slot = slot.try_into().ok()?;
        self.state
            .mt_storage
            .group_for_code(code)?
            .get(slot.raw() as usize)
            .copied()
    }
    fn current_slot(&self) -> Slot {
        Slot::from_raw(self.state.mt_storage.active_slot as i32)
    }
    fn skip(&mut self) {
        if self.skip == 0 {
            return;
        }
        Arc::make_mut(&mut self.incoming).drain(..self.skip);
        self.skip = 0;
    }
    fn next_report(&mut self, iface: &mut impl Interface) -> io::Result<Report> {
        let end = match self
            .incoming
            .iter()
            .skip(self.skip)
            .position(report_or_dropped)
        {
            Some(i) => i,
            None => self.refill(iface)?,
        };

        self.incoming
            .range(self.skip..=self.skip + end)
            .for_each(|ev| self.state.update_state(*ev));
        let skip = self.skip;
        self.skip += end + 1;

        Ok(Report {
            queue: self.incoming.clone(),
            range: skip..=skip + end,
        })
    }

    /// Looks for the next `SYN_REPORT` in the queue and returns the number of events up to the
    /// `SYN_REPORT`.
    ///
    /// If there's no `SYN_REPORT`, this will read more events from the device.
    fn next_report_len(&mut self, iface: &mut impl Interface) -> io::Result<usize> {
        self.skip();

        let idx = match self.incoming.iter().position(report_or_dropped) {
            Some(i) => Ok(i),
            None => self.refill(iface),
        };
        idx.map(|i| i + 1)
    }
    fn next_event(&mut self) -> InputEvent {
        self.skip();
        let ev = Arc::make_mut(&mut self.incoming)
            .pop_front()
            .expect("`next_event` called with no events in queue");
        self.state.update_state(ev);
        ev
    }

    /// Reads events until at least one SYN_REPORT or SYN_DROPPED is found, or reading fails.
    ///
    /// Returns the index of the SYN_x event in the queue.
    fn refill(&mut self, i: &mut impl Interface) -> io::Result<usize> {
        /// 21 * 24 bytes = 504 bytes, so that we fill a 512 B allocation size class with little waste
        /// (assuming one exists, etc.).
        const BATCH_READ_SIZE: usize = 21;
        const PLACEHOLDER: InputEvent = InputEvent::new(EventType::from_raw(0xffff), 0xffff, -1);

        self.skip();

        // This `make_mut` will not cause any clones unless `Report`s are kept alive between calls
        // (for example, because the caller is `collect()`ing the `Reports` iterator).
        // In the latter case this will make each `Report` hold on to a 512 byte allocation (or more,
        // if reports contain more events).
        let incoming = Arc::make_mut(&mut self.incoming);

        loop {
            // `VecDeque` has no `set_len` or `as_mut_ptr`, so we have to add dummy elements to read
            // into, and then remove the ones that weren't overwritten.
            let len_before = incoming.len();
            incoming.reserve(BATCH_READ_SIZE);
            incoming.extend(iter::repeat(PLACEHOLDER).take(BATCH_READ_SIZE));

            // If the queue wraps around, we might have two discontinuous destination buffers
            // available. We only write to the first and let the outer loop handle the rest.
            let (first, second) = incoming.as_mut_slices();
            let dest = if first.len() <= len_before {
                &mut second[len_before - first.len()..]
            } else {
                &mut first[len_before..]
            };
            let res = i.read(dest);

            // Truncate the queue so it only contains events we actually read.
            let count = *res.as_ref().ok().unwrap_or(&0);
            incoming.truncate(len_before + count);

            debug_assert!(
                !incoming.contains(&PLACEHOLDER),
                "should not contain placeholders: {:?}",
                incoming
            );

            res?;

            let end = match incoming.range(len_before..).position(report_or_dropped) {
                Some(i) => len_before + i,
                None => continue, // no SYN_x event, try to read more
            };
            let ev = incoming[end];
            let syn = match ev.kind() {
                EventKind::Syn(ev) => ev,
                _ => unreachable!("got invalid event at the end of a batch: {ev:?}"),
            };

            // Save the timestamp of the last event in the batch.
            self.state.last_event = ev.time();

            match syn.syn() {
                Syn::REPORT => {
                    if self.discard_events {
                        // We have to drop this batch.
                        self.discard_events = false;
                        drop(incoming.drain(..=end));
                        continue;
                    } else {
                        // We can return this batch.
                        return Ok(end);
                    }
                }
                Syn::DROPPED => {
                    // At least one event has been lost, so we have to resynchronize.
                    // According to the `libevdev` documentation, we we have to:
                    // - Drop all uncommitted events (events that weren't followed up by a `SYN_REPORT`).
                    // - Drop all *future* events until we get a `SYN_REPORT`.
                    log::debug!("SYN_DROPPED: events were lost! resyncing");
                    self.discard_events = true;
                    incoming.clear();

                    // Fetch device state and synthesize events.
                    i.resync(&mut self.state, incoming)?;

                    if !incoming.is_empty() {
                        // If `resync` generates any events, the last one is guaranteed to be a SYN_REPORT.
                        return Ok(incoming.len() - 1);
                    }

                    // We will return to normal operation once the synthetic events have been
                    // cleared out and all events until the next `SYN_REPORT` have been discarded.
                }
                _ => unreachable!("unexpected SYN event at the end of a batch: {syn:?}"),
            }
        }
    }
}

trait Interface {
    fn read(&mut self, dest: &mut [InputEvent]) -> io::Result<usize>;
    fn resync(&self, state: &mut DeviceState, queue: &mut VecDeque<InputEvent>) -> io::Result<()>;
}

impl Interface for Evdev {
    fn read(&mut self, dest: &mut [InputEvent]) -> io::Result<usize> {
        read_raw(&self.file, dest)
    }

    fn resync(&self, state: &mut DeviceState, queue: &mut VecDeque<InputEvent>) -> io::Result<()> {
        let now = Instant::now();
        let _d = on_drop(|| log::debug!("`EventReader::resync` took {:?}", now.elapsed()));

        // Clear out all events, and drain the kernel buffer too, like libevdev does.
        let mut reads = 0;

        const READ_LIMIT: usize = 16;
        const READ_SIZE: usize = 128;
        while self.is_readable()? && reads < READ_LIMIT {
            let mut out = [InputEvent::zeroed(); READ_SIZE];
            read_raw(&self.file, &mut out)?;
            reads += 1;
        }
        if reads >= READ_LIMIT {
            log::warn!("resync: kernel buffer not empty after {reads}x{READ_SIZE} reads");
        }

        state.resync_from(&DeviceState::current(self)?, queue);
        Ok(())
    }
}

/// Stores a userspace view of a device, and reads events emitted by it.
///
/// Created by [`Evdev::into_reader`].
///
/// This is the recommended way of ingesting input events from an `evdev`.
///
/// In addition to reading the raw events emitted by the device, [`EventReader`] will:
/// - Keep a view of the current device state that the user can query.
/// - Fetch the current device state on creation and when a `SYN_DROPPED` event is received
///   (indicating that one or more events have been lost due to the buffer filling up).
/// - Synthesize events so that the consumer will see an up-to-date state.
///
/// The current device state from the [`EventReader`]'s PoV can be queried via
/// [`EventReader::key_state`], [`EventReader::abs_state`], [`EventReader::slot_state`], and similar
/// methods.
/// These methods are faster than the equivalent methods on [`Evdev`], since they do not have to
/// perform a system call to fetch the data (they just return data already stored in the
/// [`EventReader`]).
/// The reader's view of the device state is automatically updated as events are pulled from it, but
/// can also be manually updated by calling [`EventReader::update`], which will pull and discard all
/// available events.
#[derive(Debug)]
pub struct EventReader {
    evdev: Evdev,
    imp: Impl,
}

impl AsFd for EventReader {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.evdev.as_fd()
    }
}

impl AsRawFd for EventReader {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.evdev.as_raw_fd()
    }
}

impl IntoRawFd for EventReader {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        self.evdev.into_raw_fd()
    }
}

impl EventReader {
    pub(crate) fn new(evdev: Evdev) -> io::Result<Self> {
        let abs_axes = evdev.supported_abs_axes()?;

        let mut this = Self {
            evdev,
            imp: Impl::new(abs_axes),
        };

        // resync to inject events that represent the current device state.
        this.imp
            .state
            .resync(&this.evdev, Arc::make_mut(&mut this.imp.incoming))?;

        Ok(this)
    }

    /// Destroys this [`EventReader`] and returns the original [`Evdev`].
    ///
    /// This will drop all input events buffered in the [`EventReader`].
    #[inline]
    pub fn into_evdev(self) -> Evdev {
        self.evdev
    }

    /// Returns a reference to the [`Evdev`] this [`EventReader`] was created from.
    #[inline]
    pub fn evdev(&self) -> &Evdev {
        &self.evdev
    }

    /// Update the local device state by reading all available events from the kernel, and
    /// discarding them.
    ///
    /// This does not block.
    ///
    /// This method can be used when the application isn't interested in processing events or
    /// reports itself, and only wants to know what the current state of the input device is.
    /// [`EventReader::update`] is potentially faster than calling [`Evdev::key_state`] and other
    /// [`Evdev`] getters, since each of the [`Evdev`] getters perform a syscall.
    ///
    /// After a call to [`EventReader::update`], the up-to-date device state can be retrieved with
    /// the [`EventReader::key_state`], [`EventReader::led_state`], and other [`EventReader`]
    /// methods without incurring any additional syscalls.
    pub fn update(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let _d = on_drop(|| log::trace!("`EventReader::update` took {:?}", now.elapsed()));

        let was_nonblocking = self.evdev.set_nonblocking(true)?;

        let mut count = 0;
        let mut reports = self.reports();
        let err = loop {
            match reports.next() {
                None => break None,
                Some(Ok(_)) => count += 1,
                Some(Err(e)) => break Some(e),
            }
        };
        log::trace!("`EventReader::update` processed {count} events");

        let res = if !was_nonblocking {
            self.evdev.set_nonblocking(false).map(drop)
        } else {
            // Avoid the syscall if the device was already in non-blocking mode.
            Ok(())
        };
        match err {
            Some(e) => Err(e),
            None => res,
        }
    }

    /// Returns a [`BitSet`] of all [`Key`]s that are currently pressed.
    #[inline]
    pub fn key_state(&self) -> &BitSet<Key> {
        &self.imp.state.keys
    }

    /// Returns a [`BitSet`] of all [`Led`]s that are currently on.
    #[inline]
    pub fn led_state(&self) -> &BitSet<Led> {
        &self.imp.state.leds
    }

    /// Returns a [`BitSet`] of all [`Sound`]s that have been requested to play.
    #[inline]
    pub fn sound_state(&self) -> &BitSet<Sound> {
        &self.imp.state.sounds
    }

    /// Returns a [`BitSet`] of all [`Switch`]es that are currently active or closed.
    #[inline]
    pub fn switch_state(&self) -> &BitSet<Switch> {
        &self.imp.state.switches
    }

    /// Returns the current value of an absolute axis.
    ///
    /// `abs` must be less than [`Abs::MT_SLOT`], or this method will panic. To access
    /// multitouch slots, use [`EventReader::slot_state`] instead.
    ///
    /// Call [`EventReader::update`], or drain incoming events using the iterator interface in order
    /// to update the multitouch slot state.
    #[inline]
    pub fn abs_state(&self, abs: Abs) -> i32 {
        self.imp.abs_state(abs)
    }

    /// Returns an iterator that yields all [`Slot`]s that have valid data in them.
    ///
    /// A [`Slot`] is considered valid if its value of [`Abs::MT_TRACKING_ID`] is non-negative.
    ///
    /// Call [`EventReader::update`], or drain incoming events using the iterator interface in order
    /// to update the multitouch slot state.
    #[inline]
    pub fn valid_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.imp.valid_slots()
    }

    /// Returns an [`Abs`] axis value for a multitouch slot.
    ///
    /// `code` must be one of the `Abs::MT_*` codes (but not [`Abs::MT_SLOT`]), as only those are
    /// associated with a multitouch slot.
    /// Non-MT [`Abs`] codes can be queried via [`EventReader::abs_state`].
    ///
    /// Returns [`None`] if `code` isn't advertised by the device (ie. the property does not exist)
    /// or if `slot` is out of range (ie. the device does not have the requested slot).
    ///
    /// If `slot` isn't valid (yielded by [`EventReader::valid_slots`]), invalid stale data may be
    /// returned.
    #[inline]
    pub fn slot_state(&self, slot: impl TryInto<Slot>, code: Abs) -> Option<i32> {
        self.imp.slot_state(slot, code)
    }

    /// Returns the currently selected multitouch slot.
    ///
    /// Events with `ABS_MT_*` code affect *this* slot, but not other slots.
    #[inline]
    pub fn current_slot(&self) -> Slot {
        self.imp.current_slot()
    }

    /// Returns an iterator over incoming events.
    ///
    /// Events read from the iterator will automatically update the state of the [`EventReader`].
    ///
    /// If the underlying device is in non-blocking mode, the iterator will return [`None`] when no
    /// more events are available.
    /// If the device is *not* in non-blocking mode, the iterator will block until more events
    /// arrive.
    ///
    /// **Note**: Retrieving an event with this iterator will remove that event from the [`Report`]
    /// it belongs to if that report is later fetched with [`EventReader::reports`].
    /// It is best to stick to either per-event or per-report processing in your program to avoid
    /// this.
    #[inline]
    pub fn events(&mut self) -> Events<'_> {
        Events {
            reader: self,
            remaining: 0,
        }
    }

    /// Returns an iterator over incoming device reports.
    ///
    /// [`Report`]s are groups of [`InputEvent`]s that belong together.
    ///
    /// If the underlying device is in non-blocking mode, the iterator will return [`None`] when no
    /// more events are available.
    /// If the device is *not* in non-blocking mode, the iterator will block until more events
    /// arrive.
    ///
    /// **Note**: Retrieving an event individually (for example, via [`EventReader::events`]) will
    /// remove that event from the [`Report`] it belongs to if that report is later fetched with
    /// [`EventReader::reports`].
    /// It is best to stick to either per-event or per-report processing in your program to avoid
    /// this.
    #[inline]
    pub fn reports(&mut self) -> Reports<'_> {
        Reports(self)
    }

    /// Returns an async iterator over incoming events.
    ///
    /// Events read from the iterator will automatically update the state of the [`EventReader`].
    ///
    /// The underlying device will be put in non-blocking mode while the returned [`AsyncEvents`]
    /// is alive (if it isn't already).
    ///
    /// When using the `"tokio"` Cargo feature, this must be called while inside a tokio context.
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async-io"))))]
    #[cfg(any(feature = "tokio", feature = "async-io"))]
    #[inline]
    pub fn async_events(&mut self) -> io::Result<AsyncEvents<'_>> {
        AsyncEvents::new(self)
    }

    /// Returns an async iterator over incoming device reports.
    ///
    /// The underlying device will be put in non-blocking mode while the returned [`AsyncReports`]
    /// is alive (if it isn't already).
    ///
    /// When using the `"tokio"` Cargo feature, this must be called while inside a tokio context.
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async-io"))))]
    #[cfg(any(feature = "tokio", feature = "async-io"))]
    #[inline]
    pub fn async_reports(&mut self) -> io::Result<AsyncReports<'_>> {
        AsyncReports::new(self)
    }

    fn next_report(&mut self) -> io::Result<Report> {
        self.imp.next_report(&mut self.evdev)
    }

    fn next_report_len(&mut self) -> io::Result<usize> {
        self.imp.next_report_len(&mut self.evdev)
    }

    fn next_event(&mut self) -> InputEvent {
        self.imp.next_event()
    }
}

fn read_raw(mut file: &File, dest: &mut [InputEvent]) -> io::Result<usize> {
    let bptr = dest.as_mut_ptr().cast::<u8>();
    let byte_buf =
        unsafe { slice::from_raw_parts_mut(bptr, mem::size_of::<InputEvent>() * dest.len()) };
    let bytes = file.read(byte_buf)?;
    debug_assert_eq!(bytes % mem::size_of::<InputEvent>(), 0);
    Ok(bytes / mem::size_of::<InputEvent>())
}

fn report_or_dropped(ev: &InputEvent) -> bool {
    match ev.kind() {
        EventKind::Syn(ev) => ev.syn() == Syn::REPORT || ev.syn() == Syn::DROPPED,
        _ => false,
    }
}

impl<'a> IntoIterator for &'a mut EventReader {
    type Item = io::Result<InputEvent>;
    type IntoIter = Events<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.events()
    }
}

impl IntoIterator for EventReader {
    type Item = io::Result<InputEvent>;
    type IntoIter = IntoEvents;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoEvents {
            reader: self,
            remaining: 0,
        }
    }
}

/// An [`Iterator`] over the events produced by an [`EventReader`].
///
/// Returned by [`EventReader::events`].
#[derive(Debug)]
pub struct Events<'a> {
    reader: &'a mut EventReader,
    /// Number of events remaining in the next report.
    ///
    /// As long as this is >0, we can pull events from the queue without checking whether they are
    /// part of a committed report (by looking for `SYN_REPORT`).
    remaining: usize,
}

impl Iterator for Events<'_> {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            self.remaining = match self.reader.next_report_len() {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return None,
                Err(e) => return Some(Err(e)),
            };
        }

        self.remaining -= 1;
        Some(Ok(self.reader.next_event()))
    }
}

/// An owning [`Iterator`] over the events produced by an [`EventReader`].
#[derive(Debug)]
pub struct IntoEvents {
    reader: EventReader,
    remaining: usize,
}

impl IntoEvents {
    /// Consumes this [`IntoEvents`] iterator and returns back the original [`EventReader`].
    #[inline]
    pub fn into_reader(self) -> EventReader {
        self.reader
    }
}

impl Iterator for IntoEvents {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            self.remaining = match self.reader.next_report_len() {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return None,
                Err(e) => return Some(Err(e)),
            };
        }

        self.remaining -= 1;
        Some(Ok(self.reader.next_event()))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, None)
    }
}

/// Iterator over device [`Report`]s.
///
/// Returned by [`EventReader::reports`].
///
/// If the [`EventReader`] is in non-blocking mode, the iterator will yield [`None`] when no events
/// are pending.
/// Subsequent calls to [`Iterator::next`] may then return [`Some`] again, if more events have
/// arrived.
#[derive(Debug)]
pub struct Reports<'a>(&'a mut EventReader);

impl<'a> Iterator for Reports<'a> {
    type Item = io::Result<Report>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next_report() {
            Ok(report) => Some(Ok(report)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// A batch of [`InputEvent`]s, terminated with a `SYN_REPORT` event.
///
/// Returned by the [`Reports`] iterator.
///
/// [`Report`]s share the [`EventReader`]'s event queue via [`Arc`] where possible. If the user code
/// collects [`Report`]s from the iterator, fetching new reports will result in new event buffers
/// being allocated. If user code only lets a single [`Report`] exist at a time, [`Arc::make_mut`]
/// allows the [`EventReader`] to avoid unnecessary allocations.
#[derive(Debug)]
pub struct Report {
    queue: Arc<VecDeque<InputEvent>>,
    range: RangeInclusive<usize>,
}

impl Report {
    /// Returns an iterator over the [`InputEvent`]s in this [`Report`].
    ///
    /// [`Report`] also implements [`IntoIterator`] to facilitate the same operation.
    #[inline]
    pub fn iter(&self) -> ReportIter<'_> {
        self.into_iter()
    }

    /// Returns the number of [`InputEvent`]s in this [`Report`].
    ///
    /// Since [`Report`]s are always terminated with a [`Syn::REPORT`] event, they always have at
    /// least one event in them, so this method will never return 0.
    #[inline]
    pub fn len(&self) -> usize {
        *self.range.end() - *self.range.start() + 1
    }
}

impl<'a> IntoIterator for &'a Report {
    type Item = InputEvent;
    type IntoIter = ReportIter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReportIter {
            queue: &self.queue,
            range: self.range.clone(),
        }
    }
}

impl IntoIterator for Report {
    type Item = InputEvent;
    type IntoIter = ReportIntoIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReportIntoIter {
            queue: self.queue,
            range: self.range.clone(),
        }
    }
}

/// An owning [`Iterator`] over the [`InputEvent`]s in a [`Report`].
///
/// Returned by the [`IntoIterator`] implementation of [`Report`].
#[derive(Debug)]
pub struct ReportIntoIter {
    queue: Arc<VecDeque<InputEvent>>,
    range: RangeInclusive<usize>,
}

impl Iterator for ReportIntoIter {
    type Item = InputEvent;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let Some(i) = self.range.next() else {
            return None;
        };
        Some(self.queue[i])
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}
impl ExactSizeIterator for ReportIntoIter {}
impl FusedIterator for ReportIntoIter {}

/// A borrowing [`Iterator`] over the [`InputEvent`]s in a [`Report`].
///
/// Returned by [`Report::iter`] and the [`IntoIterator`] implementation for `&Report`.
#[derive(Debug)]
pub struct ReportIter<'a> {
    queue: &'a VecDeque<InputEvent>,
    range: RangeInclusive<usize>,
}

impl<'a> Iterator for ReportIter<'a> {
    type Item = InputEvent;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let Some(i) = self.range.next() else {
            return None;
        };
        Some(self.queue[i])
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}
impl ExactSizeIterator for ReportIter<'_> {}
impl FusedIterator for ReportIter<'_> {}
