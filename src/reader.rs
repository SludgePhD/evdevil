//! A convenient API for robustly reading device events.

use std::{
    collections::VecDeque,
    fmt, io,
    time::{Instant, SystemTime},
};

use crate::{
    Evdev, Slot,
    batch::BatchReader,
    bits::{BitSet, BitValue},
    drop::on_drop,
    event::{
        Abs, AbsEvent, EventKind, EventType, InputEvent, Key, KeyEvent, KeyState, Led, LedEvent,
        Sound, SoundEvent, Switch, SwitchEvent, Syn, SynEvent,
    },
    raw::input::EVIOCGMTSLOTS,
};

const MAX_MT_SLOTS: i32 = 60; // matches the limit libevdev documents

/// Storage for the current multitouch state.
#[derive(Clone)]
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

    fn resync(&mut self, evdev: &Evdev, abs_axes: &BitSet<Abs>) -> io::Result<()> {
        if !abs_axes.contains(Abs::MT_SLOT) {
            return Ok(());
        }
        if !abs_axes.contains(Abs::MT_TRACKING_ID) {
            log::warn!(
                "device {} advertises support for `ABS_MT_SLOT` but not `ABS_MT_TRACKING_ID`; multitouch support will not work",
                evdev.name().unwrap_or_else(|e| e.to_string()),
            );
            *self = Self::empty();
            return Ok(());
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
        self.slots = slot_count.clamp(0, MAX_MT_SLOTS) as u32;
        self.slots = (mt_slot_info.maximum() + 1) as u32;
        self.active_slot = mt_slot_info.value().max(0) as u32;
        self.data.clear();
        self.codes = 0;

        for mt_code in Abs::MT_SLOT.raw() + 1..Abs::MAX.raw() {
            if !abs_axes.contains(Abs::from_raw(mt_code)) {
                continue;
            }

            // `mt_code` is supported; fetch its current value for all slots, appending it to `data`
            self.codes += 1;
            let start_idx = self.data.len();
            self.data
                .resize(self.data.len() + 1 + self.slots as usize, 0);
            self.data[start_idx] = mt_code.into();

            unsafe {
                EVIOCGMTSLOTS((self.slots as usize + 1) * 4)
                    .ioctl(evdev, self.data[start_idx..].as_mut_ptr().cast())?;
            }
        }
        self.data.shrink_to_fit();

        Ok(())
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

    /// Fetches the current device state, and injects synthetic events to compensate for any
    /// difference to the expected state.
    fn resync(&mut self, evdev: &Evdev, queue: &mut VecDeque<InputEvent>) -> io::Result<()> {
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

        let now = Instant::now();
        let _d = on_drop(|| log::debug!("`EventReader::resync` took {:?}", now.elapsed()));

        let len_before = queue.len();
        let mut emit = |ev: InputEvent| {
            queue.push_back(ev.with_time(self.last_event));
        };

        sync_bitset(&mut self.keys, evdev.key_state()?, |key, on| {
            emit(
                KeyEvent::new(
                    key,
                    if on {
                        KeyState::PRESSED
                    } else {
                        KeyState::RELEASED
                    },
                )
                .into(),
            );
        });
        sync_bitset(&mut self.leds, evdev.led_state()?, |led, on| {
            emit(LedEvent::new(led, on).into());
        });
        sync_bitset(&mut self.sounds, evdev.sound_state()?, |snd, playing| {
            emit(SoundEvent::new(snd, playing).into());
        });
        sync_bitset(&mut self.switches, evdev.switch_state()?, |sw, on| {
            emit(SwitchEvent::new(sw, on).into());
        });

        // Re-fetch values of all non-MT absolute axes
        for abs in self.abs_axes {
            if abs.raw() >= Abs::MT_SLOT.raw() {
                break;
            }

            let prev = self.abs[abs.raw() as usize];
            let cur = evdev.abs_info(abs)?.value();
            if prev != cur {
                emit(AbsEvent::new(abs, cur).into());
            }
        }

        if self.abs_axes.contains(Abs::MT_SLOT) {
            // Re-fetch the state of every MT slot
            self.mt_storage.resync(&evdev, &self.abs_axes)?;
        }
        // FIXME: we don't currently *emit* synthetic events for multitouch changes
        // (expectation is that the `valid_slots()` and `slot_state()` API is preferred)

        // If we emitted any synthetic events, follow up with a SYN_REPORT.
        // It's not clear if this is *strictly* necessary after a SYN_DROPPED: the kernel seems to
        // emit an empty report consisting of just a SYN_REPORT event after a SYN_DROPPED.
        // It is useful after the `EventReader` is just constructed though, since the event would
        // otherwise be missing.
        if queue.len() != len_before {
            log::debug!(
                "resync injected {} events -> adding SYN_REPORT",
                queue.len() - len_before
            );
            assert_ne!(queue.back().unwrap().event_type(), EventType::SYN);
            queue.push_back(SynEvent::new(Syn::REPORT).with_time(self.last_event));
        }

        Ok(())
    }

    /// Ingests an [`InputEvent`] and updates the local device state accordingly.
    fn update_state(&mut self, ev: InputEvent) {
        match ev.kind() {
            Some(EventKind::Abs(ev)) => {
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
            Some(EventKind::Key(ev)) => match ev.state() {
                KeyState::PRESSED => {
                    self.keys.insert(ev.key());
                }
                KeyState::RELEASED => {
                    self.keys.remove(ev.key());
                }
                _ => {}
            },
            Some(EventKind::Led(ev)) => {
                if ev.is_on() {
                    self.leds.insert(ev.led());
                } else {
                    self.leds.remove(ev.led());
                }
            }
            Some(EventKind::Switch(ev)) => {
                if ev.is_pressed() {
                    self.switches.insert(ev.switch());
                } else {
                    self.switches.remove(ev.switch());
                }
            }
            Some(EventKind::Sound(ev)) => {
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
    batch: BatchReader,
    state: DeviceState,

    /// Queues outgoing input events.
    ///
    /// Events received from the raw [`Evdev`] event stream are only committed once they are
    /// followed up by a `SYN_REPORT` event. Only then do we write an event batch to this queue.
    queue: VecDeque<InputEvent>,
    /// Whether we need to discard (instead of queuing) all events until the next `SYN_REPORT`.
    ///
    /// Set after we get a `SYN_DROPPED` to clear out incomplete reports.
    discard_events: bool,
}

impl EventReader {
    pub(crate) fn new(evdev: Evdev) -> io::Result<Self> {
        let abs_axes = evdev.supported_abs_axes()?;

        let mut this = Self {
            batch: BatchReader::new(),
            state: DeviceState::new(abs_axes),
            queue: VecDeque::new(),
            evdev,
            discard_events: false,
        };

        // resync to inject events that represent the current device state.
        this.state.resync(&this.evdev, &mut this.queue)?;

        Ok(this)
    }

    /// Destroys this [`EventReader`] and returns the original [`Evdev`].
    ///
    /// This will drop all input events buffered in the [`EventReader`].
    pub fn into_evdev(self) -> Evdev {
        self.evdev
    }

    /// Returns a reference to the [`Evdev`] this [`EventReader`] was created from.
    pub fn evdev(&self) -> &Evdev {
        &self.evdev
    }

    /// Update the local device state by reading all available events from the kernel, and
    /// discarding them.
    ///
    /// This does not block.
    pub fn update(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let _d = on_drop(|| log::trace!("`EventReader::update` took {:?}", now.elapsed()));

        let was_nonblocking = self.evdev.set_nonblocking(true)?;

        let mut count = 0;
        let mut events = self.events();
        let err = loop {
            match events.next() {
                None => break None,
                Some(Ok(_)) => count += 1,
                Some(Err(e)) => break Some(e),
            }
        };
        log::trace!("`EventReader::update` processed {count} events");

        let res = self.evdev.set_nonblocking(was_nonblocking);
        match err {
            Some(e) => Err(e),
            None => res.map(drop),
        }
    }

    /// Returns a [`BitSet`] of all [`Key`]s that are currently pressed.
    pub fn key_state(&self) -> &BitSet<Key> {
        &self.state.keys
    }

    /// Returns a [`BitSet`] of all [`Led`]s that are currently on.
    pub fn led_state(&self) -> &BitSet<Led> {
        &self.state.leds
    }

    /// Returns a [`BitSet`] of all [`Sound`]s that have been requested to play.
    pub fn sound_state(&self) -> &BitSet<Sound> {
        &self.state.sounds
    }

    /// Returns a [`BitSet`] of all [`Switch`]es that are currently active or closed.
    pub fn switch_state(&self) -> &BitSet<Switch> {
        &self.state.switches
    }

    /// Returns the current value of an absolute axis.
    ///
    /// `abs` must be less than [`Abs::MT_SLOT`], or this method will panic. To access
    /// multitouch slots, use [`EventReader::slot_state`] instead.
    ///
    /// Call [`EventReader::update`], or drain incoming events using the iterator interface in order
    /// to update the multitouch slot state.
    pub fn abs_state(&self, abs: Abs) -> i32 {
        self.state.abs[abs.raw() as usize]
    }

    /// Returns an iterator that yields all [`Slot`]s that have valid data in them.
    ///
    /// A [`Slot`] is considered valid if its value of [`Abs::MT_TRACKING_ID`] is non-negative.
    ///
    /// Call [`EventReader::update`], or drain incoming events using the iterator interface in order
    /// to update the multitouch slot state.
    pub fn valid_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.state.mt_storage.valid_slots()
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
    pub fn slot_state(&self, slot: impl TryInto<Slot>, code: Abs) -> Option<i32> {
        let slot: Slot = slot.try_into().ok()?;
        assert!(
            code.raw() > Abs::MT_SLOT.raw(),
            "`slot_state` requires an `ABS_MT_*` value above `ABS_MT_SLOT`"
        );
        self.state
            .mt_storage
            .group_for_code(code)?
            .get(slot.raw() as usize)
            .copied()
    }

    /// Returns the currently selected multitouch slot.
    ///
    /// Events with `ABS_MT_*` code affect *this* slot, but not other slots.
    pub fn current_slot(&self) -> Slot {
        Slot::from_raw(self.state.mt_storage.active_slot as i32)
    }

    /// Returns an iterator over incoming events.
    ///
    /// Events read from the iterator will automatically update the state of the [`EventReader`].
    ///
    /// If the underlying device is in non-blocking mode, the iterator will return [`None`] when no
    /// more events are available.
    /// If the device is *not* in non-blocking mode, the iterator will block until more events
    /// arrive.
    pub fn events(&mut self) -> Events<'_> {
        Events(self)
    }

    /// Fetches the next batch of events from the device.
    ///
    /// The returned [`Report`] can be iterated over to yield the events contained in the batch.
    pub fn next_report(&mut self) -> io::Result<Report<'_>> {
        if self.queue.is_empty() {
            self.refill()?;
        }

        Ok(Report {
            reader: self,
            done: false,
        })
    }

    fn refill(&mut self) -> io::Result<()> {
        loop {
            let batch = match self.batch.read(&self.evdev.file)? {
                None => continue, // read some events, but no SYN_x event
                Some(batch) => batch,
            };
            log::trace!("read batch: {:?}", batch.as_slice());

            // We've been handed a `batch` of events, the last of which is guaranteed to either be
            // a SYN_REPORT or a SYN_DROPPED.
            let ev = batch.as_slice().last().expect("got empty batch");
            let syn = match ev.kind() {
                Some(EventKind::Syn(ev)) => ev,
                _ => unreachable!("got invalid event at the end of a batch: {ev:?}"),
            };

            // Save the timestamp of the last event in the batch.
            self.state.last_event = ev.time();

            match syn.syn() {
                Syn::REPORT => {
                    if self.discard_events {
                        // We have to drop this batch.
                        self.discard_events = false;
                        continue;
                    } else {
                        // We can commit this batch.
                        for ev in batch.as_slice() {
                            self.state.update_state(*ev);
                        }
                        self.queue.extend(batch);

                        return Ok(());
                    }
                }
                Syn::DROPPED => {
                    // At least one event has been lost, so we have to resynchronize.
                    // According to the `libevdev` documentation, we we have to:
                    // - Drop all uncommitted events (events that weren't followed up by a `SYN_REPORT`).
                    // - Drop all *future* events until we get a `SYN_REPORT`.
                    log::debug!("SYN_DROPPED: events were lost! resyncing");
                    self.discard_events = true;

                    // Fetch device state and synthesize events.
                    self.state.resync(&self.evdev, &mut self.queue)?;

                    if !self.queue.is_empty() {
                        return Ok(());
                    }

                    // We will return to normal operation once the synthetic events have been
                    // cleared out and all events until the next `SYN_REPORT` have been discarded.
                }
                _ => unreachable!("unexpected SYN event at the end of a batch: {syn:?}"),
            }
        }
    }

    fn next_event(&mut self) -> Option<io::Result<InputEvent>> {
        if self.queue.is_empty() {
            // No more queued events to deliver. We've been asked to block until the next one is
            // available, so do that.
            // Incoming raw events get batch-read into `self.queue`
            match self.refill() {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return None,
                Err(e) => return Some(Err(e)),
            }
        }

        return Some(Ok(self
            .queue
            .pop_front()
            .expect("queue should not be empty after refill")));
    }
}

impl<'a> IntoIterator for &'a mut EventReader {
    type Item = io::Result<InputEvent>;
    type IntoIter = Events<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.events()
    }
}

impl IntoIterator for EventReader {
    type Item = io::Result<InputEvent>;
    type IntoIter = IntoEvents;

    fn into_iter(self) -> Self::IntoIter {
        IntoEvents(self)
    }
}

/// An [`Iterator`] over the events produced by an [`EventReader`].
///
/// Returned by [`EventReader::events`].
#[derive(Debug)]
pub struct Events<'a>(&'a mut EventReader);

impl Iterator for Events<'_> {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next_event()
    }
}

/// An owning [`Iterator`] over the events produced by an [`EventReader`].
#[derive(Debug)]
pub struct IntoEvents(EventReader);

impl IntoEvents {
    /// Consumes this [`IntoEvents`] iterator and returns back the original [`EventReader`].
    pub fn into_reader(self) -> EventReader {
        self.0
    }
}

impl Iterator for IntoEvents {
    type Item = io::Result<InputEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next_event()
    }
}

/// An iterator over a batch of [`InputEvent`]s, terminated with a `SYN_REPORT` event.
///
/// Returned by [`EventReader::next_report`].
#[derive(Debug)]
pub struct Report<'a> {
    reader: &'a mut EventReader,
    done: bool,
}

impl<'a> Iterator for Report<'a> {
    type Item = InputEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let event = self.reader.queue.pop_front().unwrap();
        if event.event_type() == EventType::SYN {
            self.done = true;
        }
        Some(event)
    }
}
