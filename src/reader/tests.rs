use std::{cmp::min, time::Duration};

use crate::{
    event::{Rel, RelEvent},
    test::check_events,
};

use super::*;

struct TestIntf {
    raw_events: Vec<InputEvent>,
}

impl Interface for TestIntf {
    fn read(&mut self, dest: &mut [InputEvent]) -> io::Result<usize> {
        let n = min(dest.len(), self.raw_events.len());
        dest[..n].copy_from_slice(&self.raw_events[..n]);
        self.raw_events.drain(..n);
        Ok(n)
    }

    fn resync(
        &self,
        _state: &mut DeviceState,
        _queue: &mut VecDeque<InputEvent>,
    ) -> io::Result<()> {
        todo!()
    }
}

struct EventReaderTest {
    imp: Impl,
    test: TestIntf,
}

impl EventReaderTest {
    fn new() -> Self {
        Self {
            imp: Impl::new(BitSet::new()),
            test: TestIntf {
                raw_events: Vec::new(),
            },
        }
    }

    fn append_events(&mut self, events: impl IntoIterator<Item = InputEvent>) {
        self.test.raw_events.extend(events);
    }

    fn next_report(&mut self) -> io::Result<Report> {
        self.imp.next_report(&mut self.test)
    }
}

#[test]
fn shared_reports() -> io::Result<()> {
    let mut reader = EventReaderTest::new();
    reader.append_events([RelEvent::new(Rel::DIAL, 0).into(), Syn::REPORT.into()]);
    reader.append_events([RelEvent::new(Rel::DIAL, 1).into(), Syn::REPORT.into()]);
    reader.append_events([RelEvent::new(Rel::DIAL, 2).into(), Syn::REPORT.into()]);

    let queue_before = Arc::as_ptr(&reader.imp.incoming);
    {
        let report = reader.next_report()?;
        let report_ptr = Arc::as_ptr(&report.queue);
        let queue_ptr = Arc::as_ptr(&reader.imp.incoming);
        assert_eq!(queue_ptr, queue_before, "queue should not be cloned");
        assert_eq!(
            report_ptr, queue_ptr,
            "queue should not be cloned for a single report"
        );
        assert_eq!(report.len(), 2);
        check_events(
            report,
            [RelEvent::new(Rel::DIAL, 0).into(), Syn::REPORT.into()],
        );
    }

    {
        let report = reader.next_report()?;
        let report_ptr = Arc::as_ptr(&report.queue);
        let queue_ptr = Arc::as_ptr(&reader.imp.incoming);
        assert_eq!(
            report_ptr, queue_ptr,
            "queue should not be cloned for the second report"
        );

        let report2 = reader.next_report()?;
        let report2_ptr = Arc::as_ptr(&report.queue);
        let queue_ptr = Arc::as_ptr(&reader.imp.incoming);

        // Multiple `Report`s existing at once will share data as long as both were already in the
        // queue when the first one was created.
        assert_eq!(
            report_ptr, report2_ptr,
            "2 reports should be able to share the queue"
        );
        assert_eq!(
            report2_ptr, queue_ptr,
            "reader queue should not be reallocated"
        );

        check_events(
            report,
            [RelEvent::new(Rel::DIAL, 1).into(), Syn::REPORT.into()],
        );
        check_events(
            report2,
            [RelEvent::new(Rel::DIAL, 2).into(), Syn::REPORT.into()],
        );
    }

    Ok(())
}

/// Functionality for multitouch tests below.
impl MtStorage {
    fn new_test(slots: u32, codes: &[Abs]) -> Self {
        let mut this = Self::empty();
        this.slots = slots;
        this.codes = codes.len().try_into().unwrap();

        let chunk_size = slots as usize + 1;
        this.data.resize(codes.len() * chunk_size, 0);

        for (chunk, code) in zip(this.data.chunks_exact_mut(chunk_size), codes) {
            chunk[0] = code.raw().into();
        }

        this
    }

    fn with_value(mut self, slot: impl TryInto<Slot>, abs: Abs, value: i32) -> Self {
        let slot: Slot = slot.try_into().ok().unwrap();
        let slot = slot.raw() as usize;
        assert!(slot < self.slots as usize);
        self.mut_group_for_code(abs).unwrap()[slot] = value;
        self
    }

    fn with_active_slot(mut self, slot: impl TryInto<Slot>) -> Self {
        self.active_slot = slot.try_into().ok().unwrap().raw() as _;
        self
    }
}

#[track_caller]
fn check_mt_resync(mut old: MtStorage, new: MtStorage, events: &[InputEvent]) {
    let mut queue = VecDeque::new();
    let last_event = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    old.resync_from(&new, &mut queue, last_event);

    assert_eq!(old, new);

    let actual = Vec::from(queue);
    check_events(actual, events.iter().copied());
}

#[test]
fn mt_resync_active_slot() {
    check_mt_resync(
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]),
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]).with_active_slot(1),
        &[AbsEvent::new(Abs::MT_SLOT, 1).into()],
    );
    check_mt_resync(
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]).with_active_slot(1),
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]),
        &[AbsEvent::new(Abs::MT_SLOT, 0).into()],
    );
}

#[test]
fn mt_resync_noop() {
    check_mt_resync(
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]),
        MtStorage::new_test(2, &[Abs::MT_POSITION_X]),
        &[],
    );
}

#[test]
fn mt_resync_slot_data() {
    // Slot 0 values change, slot 1 values don't.
    check_mt_resync(
        MtStorage::new_test(2, &[Abs::MT_TRACKING_ID, Abs::MT_POSITION_X])
            .with_value(0, Abs::MT_TRACKING_ID, -1)
            .with_value(0, Abs::MT_POSITION_X, 100)
            .with_value(1, Abs::MT_POSITION_X, 111),
        MtStorage::new_test(2, &[Abs::MT_TRACKING_ID, Abs::MT_POSITION_X])
            .with_value(0, Abs::MT_TRACKING_ID, 0)
            .with_value(1, Abs::MT_POSITION_X, 111),
        &[
            *AbsEvent::new(Abs::MT_SLOT, 0),
            *AbsEvent::new(Abs::MT_TRACKING_ID, 0),
            *AbsEvent::new(Abs::MT_POSITION_X, 0),
        ],
    );

    // Values for both slots change.
    check_mt_resync(
        MtStorage::new_test(2, &[Abs::MT_POSITION_X])
            .with_value(0, Abs::MT_POSITION_X, 111)
            .with_value(1, Abs::MT_POSITION_X, 222),
        MtStorage::new_test(2, &[Abs::MT_POSITION_X])
            .with_value(0, Abs::MT_POSITION_X, 112)
            .with_value(1, Abs::MT_POSITION_X, 223),
        &[
            *AbsEvent::new(Abs::MT_SLOT, 0),
            *AbsEvent::new(Abs::MT_POSITION_X, 112),
            *AbsEvent::new(Abs::MT_SLOT, 1),
            *AbsEvent::new(Abs::MT_POSITION_X, 223),
        ],
    );
}
