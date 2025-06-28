use std::cmp::min;

use crate::event::{Rel, RelEvent};

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

    fn next_report(&mut self) -> io::Result<Report<'_>> {
        self.imp.next_report(&mut self.test)
    }
}

fn events_eq(recv: &InputEvent, expected: &InputEvent) -> bool {
    if recv.event_type() != expected.event_type() || recv.raw_code() != expected.raw_code() {
        return false;
    }

    // Value is ignored for SYN events
    if recv.event_type() != EventType::SYN && recv.raw_value() != expected.raw_value() {
        return false;
    }
    true
}

#[track_caller]
fn check_events(actual: &[InputEvent], expected: &[InputEvent]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "expected {} events, got {actual:?}",
        expected.len()
    );
    if !zip(actual.iter(), expected.iter()).all(|(a, b)| events_eq(a, b)) {
        panic!("expected {expected:?}, got {actual:?}");
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
        let report = reader.next_report()?.to_owned();
        let report_ptr = Arc::as_ptr(&report.queue);
        let queue_ptr = Arc::as_ptr(&reader.imp.incoming);
        assert_eq!(queue_ptr, queue_before, "queue should not be cloned");
        assert_eq!(
            report_ptr, queue_ptr,
            "queue should not be cloned for a single report"
        );
        check_events(
            &report.collect::<Vec<_>>(),
            &[RelEvent::new(Rel::DIAL, 0).into(), Syn::REPORT.into()],
        );
    }

    {
        let report = reader.next_report()?.to_owned();
        let report_ptr = Arc::as_ptr(&report.queue);
        let queue_ptr = Arc::as_ptr(&reader.imp.incoming);
        assert_eq!(
            report_ptr, queue_ptr,
            "queue should not be cloned for the second report"
        );

        let report2 = reader.next_report()?.to_owned();
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
            &report.collect::<Vec<_>>(),
            &[RelEvent::new(Rel::DIAL, 1).into(), Syn::REPORT.into()],
        );
        check_events(
            &report2.collect::<Vec<_>>(),
            &[RelEvent::new(Rel::DIAL, 2).into(), Syn::REPORT.into()],
        );
    }

    Ok(())
}
