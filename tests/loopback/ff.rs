#![cfg(not(target_os = "freebsd"))] // FreeBSD does not support force-feedback (stubbed out)

use std::{collections::HashSet, error::Error, io, sync::mpsc};

use evdevil::{
    event::{EventKind, ForceFeedbackCode, Rel, RelEvent, UinputCode},
    ff::{Effect, EffectId, Rumble},
};

use crate::Tester;

struct FFTest<'a> {
    t: &'a mut Tester,
    uploaded: HashSet<EffectId>,
    playing: HashSet<EffectId>,
}

impl<'a> FFTest<'a> {
    fn new(t: &'a mut Tester) -> Self {
        Self {
            t,
            uploaded: HashSet::new(),
            playing: HashSet::new(),
        }
    }

    fn upload_effect(&mut self, effect: impl Into<Effect<'static>>) -> io::Result<EffectId> {
        self.upload_effect_impl(effect.into(), Ok(()))
    }
    fn upload_effect_error(
        &mut self,
        effect: impl Into<Effect<'static>>,
        err: io::Error,
    ) -> io::Result<EffectId> {
        self.upload_effect_impl(effect.into(), Err(err))
    }
    fn upload_effect_impl(
        &mut self,
        effect: Effect<'static>,
        res: io::Result<()>,
    ) -> io::Result<EffectId> {
        let (send, recv) = mpsc::sync_channel(1);
        self.t.with_evdev_thread(move |evdev| {
            let res = evdev.upload_ff_effect(effect);
            send.send(res).unwrap();
            Ok(())
        });

        let ours = match self.t.uinput.events().next().unwrap()?.kind() {
            EventKind::Uinput(ui) if ui.code() == UinputCode::FF_UPLOAD => {
                log::debug!("got event {ui:?}");
                self.t.uinput.ff_upload(&ui, |upl| {
                    assert_eq!(upl.effect().effect_type(), effect.effect_type());
                    assert_eq!(upl.effect().kind(), effect.kind());
                    res.map(|()| upl.effect_id())
                })
            }
            e => panic!("unexpected event: {e:?}"),
        };
        self.t.join_thread();

        // Both the `uinput` and `evdev` operations should return the same `ErrorKind`.
        let theirs = recv.recv().unwrap();
        match (ours, theirs) {
            (Ok(id1), Ok(id2)) => {
                assert_eq!(id1, id2);
                self.uploaded.insert(id1);
                log::debug!("upload complete: id = {id2:?}");
                Ok(id2)
            }
            (Err(e1), Err(e2)) => {
                if e1.kind() != io::ErrorKind::Other && e2.kind() != io::ErrorKind::Other {
                    assert_eq!(e1.kind(), e2.kind());
                }
                Err(e2)
            }
            (ours, theirs) => panic!("{ours:?} <-> {theirs:?}"),
        }
    }

    fn play_stop(&mut self, id: EffectId, play: bool) -> io::Result<()> {
        self.t.with_evdev_thread(move |evdev| {
            evdev.control_ff(id, true)?;
            Ok(())
        });

        match self.t.uinput.events().next().unwrap()?.kind() {
            EventKind::ForceFeedback(ev) => {
                assert_eq!(ev.code(), Some(ForceFeedbackCode::ControlEffect(id)));
                assert_eq!(ev.raw_value(), if play { 1 } else { 0 });
            }
            e => panic!("unexpected event: {e:?}"),
        }

        self.t.join_thread();

        if play {
            self.playing.insert(id);
        } else {
            self.playing.remove(&id);
        }

        Ok(())
    }

    fn erase_effect(&mut self, id: EffectId) -> io::Result<()> {
        self.t.with_evdev_thread(move |evdev| {
            evdev.erase_ff_effect(id)?;
            Ok(())
        });

        // uinput will always send a stop event first.
        match self.t.uinput.events().next().unwrap()?.kind() {
            EventKind::ForceFeedback(ev) => {
                assert_eq!(ev.code(), Some(ForceFeedbackCode::ControlEffect(id)));
                assert_eq!(ev.raw_value(), 0);
            }
            e => panic!("unexpected event: {e:?}"),
        }

        match self.t.uinput.events().next().unwrap()?.kind() {
            EventKind::Uinput(ui) if ui.code() == UinputCode::FF_ERASE => {
                self.t.uinput.ff_erase(&ui, |erase| {
                    assert_eq!(erase.effect_id(), id);
                    Ok(())
                })?;
            }
            e => panic!("unexpected event: {e:?}"),
        }

        self.t.join_thread();

        self.playing.remove(&id);
        self.uploaded.remove(&id);

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        // There's a weird bug or Feature™ where the kernel will appear to insert additional FF events
        // that start or stop the effect when events are submitted by subsequent tests.
        // To get around that we submit a `RelEvent` and then drain the evdev, ignoring all events.
        self.t.uinput.write(&[RelEvent::new(Rel::DIAL, 1).into()])?;
        assert!(self.t.evdev().is_readable()?);
        while self.t.evdev().is_readable()? {
            self.t.evdev().raw_events().next().unwrap()?;
        }
        Ok(())
    }
}

impl Drop for FFTest<'_> {
    fn drop(&mut self) {
        self.flush().ok();
    }
}

const EFFECT: Rumble = Rumble::new(10, 100);

#[test]
fn upload_remove() -> io::Result<()> {
    let mut t = Tester::get();
    let mut t = FFTest::new(&mut t);

    let id = t.upload_effect(EFFECT)?;
    t.erase_effect(id)?;
    Ok(())
}

#[test]
fn upload_play_remove() -> io::Result<()> {
    let mut t = Tester::get();
    let mut t = FFTest::new(&mut t);

    let id = t.upload_effect(EFFECT)?;
    t.play_stop(id, true)?;
    t.erase_effect(id)?;
    Ok(())
}

#[test]
fn upload_too_many() -> io::Result<()> {
    // Device is created with support for 2 effects.
    let mut t = Tester::get();
    let mut t = FFTest::new(&mut t);

    let id1 = t.upload_effect(EFFECT)?;
    let id2 = t.upload_effect(EFFECT)?;

    match t.t.evdev_mut().upload_ff_effect(EFFECT) {
        Err(e) if e.kind() == io::ErrorKind::StorageFull => {}
        res => panic!("unexpected result: {res:?}"),
    }

    t.erase_effect(id1)?;
    t.erase_effect(id2)?;
    Ok(())
}

#[test]
fn upload_error() -> io::Result<()> {
    let mut t = Tester::get();
    let mut t = FFTest::new(&mut t);

    // Test that `ErrorKind`s make it through the kernel unchanged.
    // (this is a best-effort conversion)
    match t.upload_effect_error(EFFECT, io::ErrorKind::Deadlock.into()) {
        Err(e) if e.kind() == io::ErrorKind::Deadlock => {}
        e => panic!("unexpected result: {e:?}"),
    }

    // Unmappable error kinds like `Other` result in `-EIO`.
    match t.upload_effect_error(EFFECT, io::ErrorKind::Other.into()) {
        Err(e) => {
            // `e` wraps a `WrappedError` here, so first... get to the bottom of this
            let mut e: &dyn Error = &e;
            while let Some(src) = e.source() {
                e = src;
            }

            let code = e
                .downcast_ref::<io::Error>()
                .expect("root cause is not a `std::io::Error`")
                .raw_os_error()
                .expect("not an OS error");
            assert_eq!(code, libc::EIO);
        }
        e => panic!("unexpected result: {e:?}"),
    }

    Ok(())
}
