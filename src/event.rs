//! Input event types and enumerations.
//!
//! All device event iterators will yield [`InputEvent`], the base event type.
//!
//! Events carry the following information:
//!
//! - **Timestamp** ([`InputEvent::time`]): The time at which the event has been inserted into the
//!   kernel buffer. The default time source is the system's real-time clock, which is also used by
//!   [`SystemTime::now`]. It can be changed by calling [`Evdev::set_clockid`].
//! - **Event Type** ([`InputEvent::event_type`]): The broad category of event. The [`EventType`]
//!   determines which of the event wrappers listed below is responsible.
//! - **Event Code** ([`InputEvent::raw_code`]): A `u16` identifying the button, axis, or other
//!   object affected by the event. This is typically *wrapped* in a type like [`Syn`], [`Key`],
//!   [`Abs`] or [`Rel`] and accessed from a specific event wrapper.
//! - **Event Value** ([`InputEvent::raw_value`]): An `i32` describing *what* happened to the object
//!   identified by the code. This can be the new state of an [`Abs`] axis, an incremental movement
//!   of a [`Rel`] axis, the new state (pressed/released/repeated) of a [`Key`] or [`Switch`], or
//!   some other value.
//!
//! [`InputEvent::kind`] can be used to obtain an [`EventKind`], which can be conveniently `match`ed
//! on to handle the different types of events.
//!
//! Devices may emit a variety of different types of events, which are all wrapped by the following
//! Rust types in this module:
//!
//! - [`SynEvent`]
//! - [`KeyEvent`]
//! - [`RelEvent`]
//! - [`AbsEvent`]
//! - [`SwitchEvent`]
//! - [`MiscEvent`]
//! - [`LedEvent`]
//! - [`RepeatEvent`]
//! - [`SoundEvent`]
//! - [`UinputEvent`]
//! - [`ForceFeedbackEvent`]
//!
//! All of these event types can be converted to [`EventKind`] and [`InputEvent`] via [`From`] and
//! [`Into`], and can be [`Deref`]erenced to obtain the [`InputEvent`] they wrap.
//!
//! # Serde support
//!
//! If the `serde` feature is enabled, implementations of [`Serialize`] and [`Deserialize`] will be
//! provided for the following types:
//!
//! - [`Abs`]
//! - [`Key`]
//! - [`Rel`]
//! - [`Misc`]
//! - [`Led`]
//! - [`Switch`]
//! - [`Sound`]
//!
//! For human-readable formats, the serde representation will use the evdev constant name if the
//! value has one (eg. `KEY_F1`, `ABS_Y`, ...), and the raw [`u16`] code if it does not.
//! Deserialization from a human-readable format will accept either.
//!
//! Note that this means that when a new key or axis name is added in a later version of this crate,
//! older versions will not be able to deserialize it.
//! `evdevil` guarantees only old->new compatibility between non-breaking versions, not new->old.
//!
//! Note also that some values have multiple names.
//! For example, [`Key::BTN_TRIGGER_HAPPY`] is the same value as [`Key::BTN_TRIGGER_HAPPY1`].
//! Deserialization will accept either name, but serialization has to pick a single name, which is
//! typically the first one with a given value, but is not generally guaranteed to be any specific
//! constant name.
//!
//! For non-human-readable formats, the raw numeric code is always used, and formats that aren't
//! self-describing (like [postcard]) are supported.
//! There is also no compatibility concern in case new key or axis names are added in later
//! versions.
//!
//! [`Evdev::set_clockid`]: crate::Evdev::set_clockid
//! [`Serialize`]: ::serde::Serialize
//! [`Deserialize`]: ::serde::Deserialize
//! [postcard]: https://github.com/jamesmunns/postcard

pub(crate) mod codes;

#[cfg(any(test, feature = "serde"))]
mod serde;

use std::fmt;
use std::ops::Deref;
use std::time::{Duration, SystemTime};

use crate::ff::{self, EffectId};
use crate::raw::input::input_event;

pub use codes::{Abs, EventType, Key, Led, Misc, Rel, Repeat, Sound, Switch, Syn, UinputCode};

/// An input event received from or sent to an *evdev*.
///
/// Use [`InputEvent::kind`] to convert it to a `match`able enum.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct InputEvent(input_event);

impl InputEvent {
    /// Creates an [`InputEvent`] from raw values.
    ///
    /// The timestamp of the event will be set to 0.
    #[inline]
    pub const fn new(ty: EventType, raw_code: u16, raw_value: i32) -> Self {
        Self(input_event {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: ty.0,
            code: raw_code,
            value: raw_value,
        })
    }

    /// Creates an [`InputEvent`] with all fields zeroed out.
    ///
    /// Useful as a dummy or filler value that will be overwritten with a "real" event soon.
    ///
    /// This results in a [`Syn::REPORT`] event.
    #[inline]
    pub const fn zeroed() -> Self {
        Self(input_event {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: 0,
            code: 0,
            value: 0,
        })
    }

    /// Changes the timestamp of `self` to the given [`SystemTime`].
    ///
    /// **Note**: [`InputEvent`] uses a `timeval` to store the timestamp, which has microsecond
    /// resolution, while [`SystemTime`] can represent nanoseconds on Unix.
    /// The value will be truncated or rounded to fit in the `timeval`.
    pub fn with_time(mut self, time: SystemTime) -> Self {
        let dur = if time >= SystemTime::UNIX_EPOCH {
            time.duration_since(SystemTime::UNIX_EPOCH).unwrap()
        } else {
            SystemTime::UNIX_EPOCH.duration_since(time).unwrap()
        };
        let sign = if time >= SystemTime::UNIX_EPOCH {
            1
        } else {
            -1
        };
        let sec = dur.as_secs();
        let usec = dur.subsec_micros();
        self.0.time.tv_sec = sec.try_into().unwrap();
        self.0.time.tv_sec *= sign;
        self.0.time.tv_usec = usec.try_into().unwrap();
        self
    }

    /// Returns the timestamp stored in the event.
    ///
    /// The clock source used to generate event timestamps can be changed by calling
    /// [`Evdev::set_clockid`].
    ///
    /// [`Evdev::set_clockid`]: crate::Evdev::set_clockid
    pub fn time(&self) -> SystemTime {
        match self.try_time() {
            Some(time) => time,
            None => {
                log::warn!(
                    "`input_event` timestamp out of range of `SystemTime`: tv_sec={} tv_usec={}",
                    self.0.time.tv_sec,
                    self.0.time.tv_usec,
                );
                SystemTime::UNIX_EPOCH
            }
        }
    }
    fn try_time(&self) -> Option<SystemTime> {
        let sec = self.0.time.tv_sec;
        let usec = self.0.time.tv_usec.clamp(0, 999_999);

        let dur = Duration::new(
            sec.unsigned_abs() as u64,
            (usec * 1000) as u32, // 999_999_000 fits in u32
        );

        if sec >= 0 {
            SystemTime::UNIX_EPOCH.checked_add(dur)
        } else {
            SystemTime::UNIX_EPOCH.checked_sub(dur)
        }
    }

    /// Returns the [`EventKind`] this [`InputEvent`] encodes.
    ///
    /// [`EventKind`] is a matchable, type-safe `enum` which is intended to be the primary way most
    /// applications examine input events.
    ///
    /// [`EventKind`] is `#[non_exhaustive]`, so matching on it requires a wildcard arm that will
    /// catch any events that don't have a specific [`EventKind`] variant.
    /// Future versions of `evdevil` might add new variants to catch those events.
    #[inline]
    pub fn kind(&self) -> EventKind {
        match self.event_type() {
            EventType::SYN => SynEvent(*self).into(),
            EventType::KEY => KeyEvent(*self).into(),
            EventType::REL => RelEvent(*self).into(),
            EventType::ABS => AbsEvent(*self).into(),
            EventType::SW => SwitchEvent(*self).into(),
            EventType::MSC => MiscEvent(*self).into(),
            EventType::LED => LedEvent(*self).into(),
            EventType::REP => RepeatEvent(*self).into(),
            EventType::SND => SoundEvent(*self).into(),
            EventType::UINPUT => UinputEvent(*self).into(),
            EventType::FF => ForceFeedbackEvent(*self).into(),
            _ => EventKind::Other(*self),
        }
    }

    /// Returns the [`EventType`] of this event.
    #[inline]
    pub fn event_type(&self) -> EventType {
        EventType(self.0.type_)
    }

    /// Returns the raw *event code* field.
    ///
    /// The *code* of an [`InputEvent`] generally describes what entity the event is about.
    /// Depending on the type of event, it can describe a key, axis, sound, LED, or other entity.
    #[inline]
    pub fn raw_code(&self) -> u16 {
        self.0.code
    }

    /// Returns the raw *event value* field.
    ///
    /// The *value* of an [`InputEvent`] describes the new state of the key, axis, LED, or other
    /// entity.
    #[inline]
    pub fn raw_value(&self) -> i32 {
        self.0.value
    }
}

impl fmt::Debug for InputEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            EventKind::Other(_) => f
                .debug_struct("InputEvent")
                .field("time", &self.time())
                .field("type", &self.event_type())
                .field("code", &self.raw_code())
                .field("value", &self.raw_value())
                .finish(),
            kind => kind.fmt(f),
        }
    }
}

macro_rules! event_wrappers {
    ( $(
        $(#[$attr:meta])*
        pub struct $name:ident in $variant:ident;
    )* ) => {
        $(
            $( #[$attr] )*
            #[derive(Clone, Copy, PartialEq, Eq)]
            pub struct $name(InputEvent);

            impl From<$name> for EventKind {
                #[inline]
                fn from(value: $name) -> Self {
                    Self::$variant(value)
                }
            }

            impl From<$name> for InputEvent {
                #[inline]
                fn from(value: $name) -> Self {
                    value.0
                }
            }

            impl Deref for $name {
                type Target = InputEvent;

                #[inline]
                fn deref(&self) -> &InputEvent {
                    &self.0
                }
            }
        )*

        /// Enumeration of event types.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum EventKind {
            $(
                $( #[$attr] )*
                $variant($name),
            )*

            /// Fallback variant for unknown events.
            ///
            /// This cannot be matched on by user code. Future versions of `evdevil` might add new
            /// variants to [`EventKind`] that match events previously captured as `Other`.
            #[non_exhaustive] // prevents construction and use in patterns
            Other(InputEvent),
        }

        impl From<EventKind> for InputEvent {
            #[inline]
            fn from(kind: EventKind) -> InputEvent {
                match kind {
                    $(
                        EventKind::$variant(it) => *it,
                    )*
                    EventKind::Other(ev) => ev,
                }
            }
        }
    };
}

event_wrappers! {
    /// A synchronization event.
    pub struct SynEvent in Syn;
    /// A key press/release/repeat event.
    pub struct KeyEvent in Key;
    /// A relative axis change.
    pub struct RelEvent in Rel;
    /// An absolute axis change.
    pub struct AbsEvent in Abs;
    /// A switch state change.
    pub struct SwitchEvent in Switch;
    /// Miscellaneous management events.
    pub struct MiscEvent in Misc;
    /// Reports or changes the state of device LEDs.
    pub struct LedEvent in Led;
    /// The key repeat settings have been changed.
    ///
    /// **Note**: This event is *not* used to signal key repeats.
    /// Key repeat events are sent as [`KeyEvent`]s with a value of 2 ([`KeyState::REPEAT`]).
    pub struct RepeatEvent in Repeat;
    /// Plays simple sounds on the device.
    pub struct SoundEvent in Sound;
    /// Internal events sent to [`UinputDevice`]s.
    ///
    /// [`UinputDevice`]: crate::uinput::UinputDevice
    pub struct UinputEvent in Uinput;
    /// Starts or stops previously uploaded force-feedback effects.
    pub struct ForceFeedbackEvent in ForceFeedback;
}

impl SynEvent {
    #[inline]
    pub fn new(syn: Syn) -> Self {
        Self(InputEvent::new(EventType::SYN, syn.0, 0))
    }

    /// Returns the event code as a [`Syn`] (the specific kind of `SYN` event).
    #[inline]
    pub fn syn(&self) -> Syn {
        Syn(self.raw_code())
    }
}
impl fmt::Debug for SynEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SynEvent")
            .field("time", &self.time())
            .field("syn", &self.syn())
            .field("value", &self.raw_value())
            .finish()
    }
}
impl From<Syn> for SynEvent {
    #[inline]
    fn from(syn: Syn) -> Self {
        Self::new(syn)
    }
}
impl From<Syn> for EventKind {
    #[inline]
    fn from(value: Syn) -> Self {
        Self::Syn(value.into())
    }
}
impl From<Syn> for InputEvent {
    #[inline]
    fn from(value: Syn) -> Self {
        SynEvent::new(value).into()
    }
}

impl KeyEvent {
    #[inline]
    pub fn new(key: Key, state: KeyState) -> Self {
        Self(InputEvent::new(EventType::KEY, key.0, state.0))
    }

    /// Returns the [`Key`] code that has been pressed/released/repeated.
    #[inline]
    pub fn key(&self) -> Key {
        Key(self.raw_code())
    }

    /// Returns the state of the key.
    ///
    /// This will be [`KeyState::RELEASED`] if the key has been released, [`KeyState::PRESSED`] if
    /// it has just been pressed, and [`KeyState::REPEAT`] if the key has been held down after being
    /// pressed.
    #[inline]
    pub fn state(&self) -> KeyState {
        KeyState(self.raw_value())
    }
}
impl fmt::Debug for KeyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyEvent")
            .field("time", &self.time())
            .field("key", &self.key())
            .field("state", &self.state())
            .finish()
    }
}

ffi_enum! {
    /// State of a [`Key`], stored as the value of a [`KeyEvent`].
    ///
    /// Returned by [`KeyEvent::state`].
    pub enum KeyState: i32 {
        /// The key used to be pressed and has now been released.
        RELEASED = 0,
        /// The key used to be released and has now been pressed.
        PRESSED = 1,
        /// The key is pressed, and has been held down long enough to generate a repeat event.
        REPEAT = 2,
    }
}
impl fmt::Debug for KeyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => f.write_str(name),
            None => write!(f, "KeyState({:#?})", self.0),
        }
    }
}

impl RelEvent {
    #[inline]
    pub fn new(rel: Rel, value: i32) -> Self {
        Self(InputEvent::new(EventType::REL, rel.0, value))
    }

    /// Returns the [`Rel`] axis identifier of this event.
    #[inline]
    pub fn rel(&self) -> Rel {
        Rel(self.raw_code())
    }

    /// Returns the value by which the axis has moved.
    #[inline]
    pub fn value(&self) -> i32 {
        self.raw_value()
    }
}
impl fmt::Debug for RelEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelEvent")
            .field("time", &self.time())
            .field("rel", &self.rel())
            .field("value", &self.value())
            .finish()
    }
}

impl AbsEvent {
    #[inline]
    pub fn new(abs: Abs, value: i32) -> Self {
        Self(InputEvent::new(EventType::ABS, abs.0, value))
    }

    #[inline]
    pub fn abs(&self) -> Abs {
        Abs(self.raw_code())
    }

    #[inline]
    pub fn value(&self) -> i32 {
        self.raw_value()
    }
}
impl fmt::Debug for AbsEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbsEvent")
            .field("time", &self.time())
            .field("abs", &self.abs())
            .field("value", &self.value())
            .finish()
    }
}

impl SwitchEvent {
    #[inline]
    pub fn new(switch: Switch, on: bool) -> Self {
        Self(InputEvent::new(
            EventType::SW,
            switch.0,
            if on { 1 } else { 0 },
        ))
    }

    #[inline]
    pub fn switch(&self) -> Switch {
        Switch(self.raw_code())
    }

    #[inline]
    pub fn is_pressed(&self) -> bool {
        self.raw_value() != 0
    }
}
impl fmt::Debug for SwitchEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwitchEvent")
            .field("time", &self.time())
            .field("switch", &self.switch())
            .field("pressed", &self.is_pressed())
            .finish()
    }
}

impl MiscEvent {
    #[inline]
    pub fn new(misc: Misc, value: i32) -> Self {
        Self(InputEvent::new(EventType::MSC, misc.0, value))
    }

    /// Returns the event code (the type of *misc* event).
    #[inline]
    pub fn misc(&self) -> Misc {
        Misc(self.raw_code())
    }
}
impl fmt::Debug for MiscEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MiscEvent")
            .field("time", &self.time())
            .field("misc", &self.misc())
            .field("value", &self.raw_value())
            .finish()
    }
}

impl LedEvent {
    #[inline]
    pub fn new(led: Led, on: bool) -> Self {
        Self(InputEvent::new(
            EventType::LED,
            led.0,
            if on { 1 } else { 0 },
        ))
    }

    #[inline]
    pub fn led(&self) -> Led {
        Led(self.raw_code())
    }

    #[inline]
    pub fn is_on(&self) -> bool {
        !self.is_off()
    }

    #[inline]
    pub fn is_off(&self) -> bool {
        self.0.raw_value() == 0
    }
}
impl fmt::Debug for LedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LedEvent")
            .field("time", &self.time())
            .field("led", &self.led())
            .field("state", &self.is_on())
            .finish()
    }
}

impl RepeatEvent {
    #[inline]
    pub fn new(repeat: Repeat, value: u32) -> Self {
        Self(InputEvent::new(EventType::REP, repeat.0, value as i32))
    }

    /// Returns the type of [`Repeat`] setting to be adjusted or reported by this event.
    #[inline]
    pub fn repeat(&self) -> Repeat {
        Repeat(self.raw_code())
    }

    #[inline]
    pub fn value(&self) -> u32 {
        self.raw_value() as u32
    }
}
impl fmt::Debug for RepeatEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepeatEvent")
            .field("time", &self.time())
            .field("repeat", &self.repeat())
            .field("value", &self.raw_value())
            .finish()
    }
}

impl SoundEvent {
    #[inline]
    pub fn new(sound: Sound, playing: bool) -> Self {
        Self(InputEvent::new(
            EventType::SND,
            sound.0,
            if playing { 1 } else { 0 },
        ))
    }

    /// Returns the [`Sound`] this event wants to play.
    #[inline]
    pub fn sound(&self) -> Sound {
        Sound(self.raw_code())
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.raw_value() != 0
    }
}
impl fmt::Debug for SoundEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoundEvent")
            .field("time", &self.time())
            .field("sound", &self.sound())
            .field("value", &self.raw_value())
            .finish()
    }
}

impl UinputEvent {
    #[inline]
    pub fn code(&self) -> UinputCode {
        UinputCode(self.raw_code())
    }
}
impl fmt::Debug for UinputEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UinputEvent")
            .field("time", &self.time())
            .field("code", &self.code())
            .field("value", &self.raw_value())
            .finish()
    }
}

impl ForceFeedbackEvent {
    /// Creates a [`ForceFeedbackEvent`] that starts or stops an effect.
    #[inline]
    pub fn new_control_effect(effect: EffectId, active: bool) -> Self {
        Self(InputEvent::new(
            EventType::FF,
            effect.0 as u16,
            if active { 1 } else { 0 },
        ))
    }

    // TODO: naming of these ctors
    #[inline]
    pub fn new_set_gain(gain: u16) -> Self {
        Self(InputEvent::new(
            EventType::FF,
            ff::Feature::GAIN.0,
            gain.into(),
        ))
    }

    #[inline]
    pub fn new_set_autocenter(autocenter: u16) -> Self {
        Self(InputEvent::new(
            EventType::FF,
            ff::Feature::AUTOCENTER.0,
            autocenter.into(),
        ))
    }

    #[inline]
    pub fn code(&self) -> Option<ForceFeedbackCode> {
        const FF_GAIN: u16 = ff::Feature::GAIN.0;
        const FF_AUTOCENTER: u16 = ff::Feature::AUTOCENTER.0;

        match self.raw_code() {
            id if id < FF_GAIN => Some(ForceFeedbackCode::ControlEffect(EffectId(id as _))),
            FF_GAIN => Some(ForceFeedbackCode::SetGain),
            FF_AUTOCENTER => Some(ForceFeedbackCode::SetAutocenter),
            _ => None,
        }
    }
}

impl fmt::Debug for ForceFeedbackEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForceFeedbackEvent")
            .field("time", &self.time())
            .field("code", &self.code())
            .field("value", &self.raw_value())
            .finish()
    }
}

/// Code of a [`ForceFeedbackEvent`].
///
/// Returned by [`ForceFeedbackEvent::code`].
///
/// Describes what the intent of the event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ForceFeedbackCode {
    /// Controls the force feedback effect with the given [`EffectId`].
    ///
    /// This may be sent with an unassigned or invalid [`EffectId`]. Consumers should ignore events
    /// in that case.
    ///
    /// The event value controls whether the effect should play (0 or 1).
    ControlEffect(EffectId),

    /// Set the master gain of the device.
    ///
    /// The event value encodes the gain as a fraction of 65535.
    SetGain,

    /// Set the autocenter value of the device.
    ///
    /// The event value encodes the autocenter power as a fraction of 65535.
    SetAutocenter,
}

ffi_enum! {
    /// Multi-touch contact tool.
    ///
    /// This is used as the value of [`Abs::MT_TOOL_TYPE`] events.
    pub enum MtToolType: i32 {
        FINGER = 0x00,
        PEN    = 0x01,
        PALM   = 0x02,
        DIAL   = 0x0a,
        MAX    = 0x0f,
    }
}

impl MtToolType {
    #[inline]
    pub const fn from_raw(ty: i32) -> Self {
        Self(ty)
    }

    #[inline]
    pub const fn raw(self) -> i32 {
        self.0
    }
}
impl fmt::Debug for MtToolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => write!(f, "MT_TOOL_{name}"),
            None => write!(f, "MtToolType({:#?})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        const EV: InputEvent = InputEvent::zeroed();

        let epoch = EV.with_time(SystemTime::UNIX_EPOCH);
        assert_eq!(epoch.0.time.tv_sec, 0);
        assert_eq!(epoch.0.time.tv_usec, 0);

        // `timeval` stores a `time_t` and a `suseconds_t`; the latter is guaranteed to be signed and
        // capable of storing `-1`. We match the glibc behavior where we require the value to be in
        // the valid range; if it isn't, it is clamped.
        let mut negative_micros = EV;
        negative_micros.0.time.tv_usec = -1;
        assert_eq!(
            negative_micros.time(),
            SystemTime::UNIX_EPOCH,
            "should saturate to `UNIX_EPOCH`",
        );
        assert_eq!(
            negative_micros.time(),
            EV.with_time(SystemTime::UNIX_EPOCH).time(),
        );

        let mut before_epoch = EV;
        before_epoch.0.time.tv_sec = -1;
        assert_eq!(
            before_epoch.time(),
            SystemTime::UNIX_EPOCH - Duration::from_secs(1),
        );
        assert_eq!(
            before_epoch.time(),
            EV.with_time(SystemTime::UNIX_EPOCH - Duration::from_secs(1))
                .time()
        );

        let mut after_epoch = EV;
        after_epoch.0.time.tv_sec = 1_000_000;
        assert_eq!(
            after_epoch.time(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
        );
        assert_eq!(
            after_epoch.time(),
            EV.with_time(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))
                .time()
        );
    }
}
