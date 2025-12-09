//! Force-feedback support.
//!
//! `evdev` force feedback support is modeled after the USB Physical Input Device (PID) Class.
//!
//! In general, there is no guarantee that any given driver or device will support all features or
//! respect all [`Effect`] fields.
//! If you intend to craft immersive experiences with haptic feedback, per-device testing may be
//! necessary.
//!
//! Effects are organized as follows:
//!
//! - [`Effect`]
//!   - [`Rumble`]
//!   - [`Periodic`]
//!   - [`Constant`]
//!   - [`Ramp`]
//!   - Per-axis effects that wrap [`Condition`]
//!     - [`Spring`]
//!     - [`Friction`]
//!     - [`Damper`]
//!     - [`Inertia`]
//!
//! All effect types can be represented as [`Effect`], and also in a matchable enum [`EffectKind`].
//! The usual conversions via [`Into`]/[`From`] are supported.
//!
//! # Usage
//!
//! Support for force-feedback features can be queried via [`Evdev::supported_ff_features`].
//! The number of supported effects that can be stored at the same time can be queried via
//! [`Evdev::supported_ff_effects`].
//!
//! An appropriate effect can then be uploaded by calling [`Evdev::upload_ff_effect`], returning
//! the [`EffectId`] assigned by the kernel.
//!
//! The uploaded effect can then be triggered via [`Evdev::control_ff`], or erased by calling
//! [`Evdev::erase_ff_effect`].
//!
//! [`Evdev::supported_ff_features`]: crate::Evdev::supported_ff_features
//! [`Evdev::supported_ff_effects`]: crate::Evdev::supported_ff_effects
//! [`Evdev::upload_ff_effect`]: crate::Evdev::upload_ff_effect
//! [`Evdev::control_ff`]: crate::Evdev::control_ff
//! [`Evdev::erase_ff_effect`]: crate::Evdev::erase_ff_effect

use std::{
    fmt,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    slice,
};

use crate::{
    event::Key,
    raw::input::{
        ff_condition_effect, ff_constant_effect, ff_effect, ff_envelope, ff_periodic_effect,
        ff_ramp_effect, ff_replay, ff_rumble_effect, ff_trigger,
    },
};

ffi_enum! {
    /// Force feedback feature flags.
    ///
    /// An alluring alliteration.
    ///
    /// These feature flags can be queried with [`Evdev::supported_ff_features`] and indicate
    /// support for specific force-feedback effect types, waveforms, or control mechanisms.
    ///
    /// [`Evdev::supported_ff_features`]: crate::Evdev::supported_ff_features
    pub enum Feature: u16 {
        /// Supports [`Rumble`] effects.
        RUMBLE     = 0x50,
        /// Supports [`Periodic`] effects.
        ///
        /// The supported waveforms are listed as separate feature flags.
        PERIODIC   = 0x51,
        /// Supports [`Constant`] effects.
        CONSTANT   = 0x52,
        /// Supports [`Spring`] effects.
        SPRING     = 0x53,
        /// Supports [`Friction`] effects.
        FRICTION   = 0x54,
        /// Supports [`Damper`] effects.
        DAMPER     = 0x55,
        /// Supports [`Inertia`] effects.
        INERTIA    = 0x56,
        /// Supports [`Ramp`] effects.
        RAMP       = 0x57,

        /// [`Periodic`] effect supports [`Waveform::SQUARE`].
        SQUARE     = 0x58,
        /// [`Periodic`] effect supports [`Waveform::TRIANGLE`].
        TRIANGLE   = 0x59,
        /// [`Periodic`] effect supports [`Waveform::SINE`].
        SINE       = 0x5a,
        /// [`Periodic`] effect supports [`Waveform::SAW_UP`].
        SAW_UP     = 0x5b,
        /// [`Periodic`] effect supports [`Waveform::SAW_DOWN`].
        SAW_DOWN   = 0x5c,
        /// [`Periodic`] effect supports [`Waveform::CUSTOM`].
        CUSTOM     = 0x5d,

        /// Device supports setting a global force-feedback gain (via [`Evdev::set_ff_gain`]).
        ///
        /// [`Evdev::set_ff_gain`]: crate::Evdev::set_ff_gain
        GAIN       = 0x60,

        /// Device supports configuring an auto-center feature (via [`Evdev::set_ff_autocenter`]).
        ///
        /// [`Evdev::set_ff_autocenter`]: crate::Evdev::set_ff_autocenter
        AUTOCENTER = 0x61,
    }
}
impl Feature {
    const MAX: Self = Self(0x7f);
}
bitvalue!(Feature);

impl Feature {
    #[inline]
    pub const fn from_raw(code: u16) -> Self {
        Self(code)
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl fmt::Debug for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => write!(f, "FF_{name}"),
            None => write!(f, "Feature({:#?})", self.0),
        }
    }
}

ffi_enum! {
    /// A force-feedback effect type.
    ///
    /// Every effect type has a corresponding [`Feature`] that indicates support for it, as well as
    /// a type in this module that can store the effect parameters.
    pub enum EffectType: u16 {
        RUMBLE   = Feature::RUMBLE.0,
        PERIODIC = Feature::PERIODIC.0,
        CONSTANT = Feature::CONSTANT.0,
        SPRING   = Feature::SPRING.0,
        FRICTION = Feature::FRICTION.0,
        DAMPER   = Feature::DAMPER.0,
        INERTIA  = Feature::INERTIA.0,
        RAMP     = Feature::RAMP.0,
    }
}
impl fmt::Debug for EffectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => write!(f, "FF_{name}"),
            None => write!(f, "EffectType({:#?})", self.0),
        }
    }
}

ffi_enum! {
    /// List of waveform types for [`Periodic`] effects.
    pub enum Waveform: u16 {
        SQUARE   = Feature::SQUARE.0,
        TRIANGLE = Feature::TRIANGLE.0,
        SINE     = Feature::SINE.0,
        SAW_UP   = Feature::SAW_UP.0,
        SAW_DOWN = Feature::SAW_DOWN.0,

        /// Custom waveform data.
        ///
        /// This can be created via [`Periodic::custom`].
        CUSTOM   = Feature::CUSTOM.0,
    }
}
impl fmt::Debug for Waveform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant_name() {
            Some(name) => write!(f, "FF_{name}"),
            None => write!(f, "Waveform({:#?})", self.0),
        }
    }
}

/// Identifier for uploaded effects.
///
/// This ID type is used to refer to the uploaded effects and can be used to trigger, stop, or erase
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectId(pub(crate) i16);

/// Configures which button triggers an effect (if any).
///
/// The interval limits how often the effect can be triggered.
///
/// This is ignored by most devices.
/// You should only rely on this functionality if a device matches a list of known-working devices.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Trigger(ff_trigger);

// TODO: find a device that actually supports this, and test it

impl Trigger {
    /// No trigger button.
    #[inline]
    pub fn none() -> Self {
        Self(ff_trigger {
            button: 0,
            interval: 0,
        })
    }

    /// Good luck.
    #[inline]
    pub fn new(button: Key, interval: u16) -> Self {
        Self(ff_trigger {
            button: button.raw(),
            interval,
        })
    }

    /// Returns the button that triggers the effect.
    ///
    /// If no trigger is assigned, this will be [`Key::KEY_RESERVED`].
    #[inline]
    pub fn button(&self) -> Key {
        Key::from_raw(self.0.button)
    }

    /// Returns the interval between triggers in milliseconds.
    #[inline]
    pub fn interval(&self) -> u16 {
        self.0.interval
    }
}
impl Default for Trigger {
    #[inline]
    fn default() -> Self {
        Self::none()
    }
}
impl fmt::Debug for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trigger")
            .field("button", &self.button())
            .field("interval", &self.interval())
            .finish()
    }
}

/// Configures how an [`Effect`] should be scheduled.
///
/// Appears to be ignored by most devices.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Replay(ff_replay);

impl Replay {
    pub fn new(length: u16, delay: u16) -> Self {
        Self(ff_replay { length, delay })
    }

    pub fn length(&self) -> u16 {
        self.0.length
    }

    pub fn delay(&self) -> u16 {
        self.0.delay
    }
}

impl fmt::Debug for Replay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Replay")
            .field("length", &self.length())
            .field("delay", &self.delay())
            .finish()
    }
}

/// An effect envelope.
///
/// Effect intensity can be faded in and out by configuring the envelope.
///
/// There is no guarantee that any given driver or device will respect these settings.
///
/// Fade and attack levels are relative to the magnitude of the modulated effect.
/// Their maximum value is `0x7fff` (higher values are treated the same as `0x7fff`), representing
/// 100% of the effect's intensity.
///
/// ```text
/// atk. len.               fade len.
/// |------|               |---------|
///        +---------------+  -  -  -  -  -  -  - effect magnitude/intensity
///       /                 \
///      /                   \
///     /                     \
///    /                       \
///   /                         \
///  /                           \
/// /     _                       \
/// |     |                        \
/// |     | atk.                 _  \
/// |     | lvl.            fade |  |
/// |     |                 lvl. |  |
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Envelope(ff_envelope);

impl Envelope {
    /// Returns a zeroed-out default [`Envelope`] structure.
    ///
    /// This envelope configures no fading: the effect will start at its configured intensity
    /// immediately, and end abruptly.
    #[inline]
    pub fn new() -> Self {
        Self(ff_envelope {
            attack_length: 0,
            attack_level: 0,
            fade_length: 0,
            fade_level: 0,
        })
    }

    /// Effect will be faded in for `ms` milliseconds before reaching its full intensity.
    #[inline]
    pub fn with_attack_length(mut self, ms: u16) -> Self {
        self.0.attack_length = ms;
        self
    }

    /// Effect fade-in will start with intensity `level`.
    #[inline]
    pub fn with_attack_level(mut self, level: u16) -> Self {
        self.0.attack_level = level;
        self
    }

    /// Effect will fade out for `ms` milliseconds before stopping.
    #[inline]
    pub fn with_fade_length(mut self, ms: u16) -> Self {
        self.0.fade_length = ms;
        self
    }

    /// Effect fade-out will stop at intensity `level`.
    #[inline]
    pub fn with_fade_level(mut self, level: u16) -> Self {
        self.0.fade_level = level;
        self
    }

    #[inline]
    pub fn attack_length(&self) -> u16 {
        self.0.attack_length
    }

    #[inline]
    pub fn attack_level(&self) -> u16 {
        self.0.attack_level
    }

    #[inline]
    pub fn fade_length(&self) -> u16 {
        self.0.fade_length
    }

    #[inline]
    pub fn fade_level(&self) -> u16 {
        self.0.fade_level
    }
}

impl Default for Envelope {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope")
            .field("attack_length", &self.attack_length())
            .field("attack_level", &self.attack_level())
            .field("fade_length", &self.fade_length())
            .field("fade_level", &self.fade_level())
            .finish()
    }
}

/// A force-feedback effect description.
///
/// Primarily created from the more specific force-feedback types in this module using [`From`].
///
/// The `'a` lifetime is only used for [`Periodic`] effects with [`Waveform::CUSTOM`] (created via
/// [`Periodic::custom`]).
/// It can be `'static` for all other effects.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Effect<'a> {
    pub(crate) raw: ff_effect,
    _p: PhantomData<&'a ()>,
}

impl Effect<'_> {
    /// Creates an invalid effect. For use only by the [`From`] impls.
    fn null() -> Self {
        let mut this: Self = unsafe { mem::zeroed() };
        // -1 is the right ID to use when uploading a new effect
        this.raw.id = -1;
        this
    }

    #[inline]
    pub fn effect_type(&self) -> EffectType {
        EffectType(self.raw.type_)
    }

    #[inline]
    pub fn id(&self) -> EffectId {
        EffectId(self.raw.id)
    }

    /// Changes the [`EffectId`] stored in this [`Effect`].
    ///
    /// By default, effects use ID `-1`, which is appropriate when uploading a new effect to a
    /// device (the input subsystem will allocate an ID for the effect).
    ///
    /// The ID can be set to a existing [`EffectId`] in order to reconfigure an already uploaded
    /// effect.
    #[inline]
    pub fn with_id(mut self, id: EffectId) -> Self {
        self.raw.id = id.0;
        self
    }

    #[inline]
    pub fn direction(&self) -> u16 {
        self.raw.direction
    }

    #[inline]
    pub fn with_direction(mut self, dir: u16) -> Self {
        self.raw.direction = dir;
        self
    }

    #[inline]
    pub fn trigger(&self) -> Trigger {
        Trigger(self.raw.trigger)
    }

    #[inline]
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.raw.trigger = trigger.0;
        self
    }

    #[inline]
    pub fn replay(&self) -> Replay {
        Replay(self.raw.replay)
    }

    #[inline]
    pub fn with_replay(mut self, replay: Replay) -> Self {
        self.raw.replay = replay.0;
        self
    }

    pub fn kind(&self) -> Option<EffectKind<'_>> {
        // Safety relies on making it impossible to construct `Effect`s with a mismatched type.
        unsafe {
            Some(match self.effect_type() {
                EffectType::CONSTANT => EffectKind::Constant(Constant(self.raw.u.constant)),
                EffectType::RAMP => EffectKind::Ramp(Ramp(self.raw.u.ramp)),
                EffectType::PERIODIC => EffectKind::Periodic(Periodic {
                    raw: self.raw.u.periodic,
                    _p: PhantomData,
                }),
                EffectType::RUMBLE => EffectKind::Rumble(Rumble(self.raw.u.rumble)),
                EffectType::SPRING => {
                    let [a, b] = self.raw.u.condition;
                    EffectKind::Spring([Spring(Condition(a)), Spring(Condition(b))])
                }
                EffectType::FRICTION => {
                    let [a, b] = self.raw.u.condition;
                    EffectKind::Friction([Friction(Condition(a)), Friction(Condition(b))])
                }
                EffectType::DAMPER => {
                    let [a, b] = self.raw.u.condition;
                    EffectKind::Damper([Damper(Condition(a)), Damper(Condition(b))])
                }
                EffectType::INERTIA => {
                    let [a, b] = self.raw.u.condition;
                    EffectKind::Inertia([Inertia(Condition(a)), Inertia(Condition(b))])
                }
                _ => return None,
            })
        }
    }
}

impl fmt::Debug for Effect<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Effect")
            .field("type", &self.effect_type())
            .field("id", &self.id())
            .field("direction", &self.direction())
            .field("trigger", &self.trigger())
            .field("replay", &self.replay())
            .field("kind", &self.kind())
            .finish()
    }
}

impl<'a> From<EffectKind<'a>> for Effect<'a> {
    #[inline]
    fn from(value: EffectKind<'a>) -> Self {
        match value {
            EffectKind::Constant(constant) => Self::from(constant),
            EffectKind::Ramp(ramp) => Self::from(ramp),
            EffectKind::Periodic(periodic) => Self::from(periodic),
            EffectKind::Rumble(rumble) => Self::from(rumble),
            EffectKind::Spring(springs) => Self::from(springs),
            EffectKind::Friction(friction) => Self::from(friction),
            EffectKind::Damper(dampers) => Self::from(dampers),
            EffectKind::Inertia(inertias) => Self::from(inertias),
        }
    }
}

impl From<Constant> for Effect<'_> {
    #[inline]
    fn from(value: Constant) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.constant = value.0;
        effect.raw.type_ = EffectType::CONSTANT.0;
        effect
    }
}

impl From<Ramp> for Effect<'_> {
    #[inline]
    fn from(value: Ramp) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.ramp = value.0;
        effect.raw.type_ = EffectType::RAMP.0;
        effect
    }
}

impl<'a> From<Periodic<'a>> for Effect<'a> {
    #[inline]
    fn from(value: Periodic<'a>) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.periodic = value.raw;
        effect.raw.type_ = EffectType::PERIODIC.0;
        effect
    }
}

impl From<Rumble> for Effect<'_> {
    #[inline]
    fn from(value: Rumble) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.rumble = value.0;
        effect.raw.type_ = EffectType::RUMBLE.0;
        effect
    }
}

impl From<Spring> for Effect<'_> {
    #[inline]
    fn from(value: Spring) -> Self {
        Self::from([value, value])
    }
}
impl From<[Spring; 2]> for Effect<'_> {
    #[inline]
    fn from([a, b]: [Spring; 2]) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.condition = [a.0.0, b.0.0];
        effect.raw.type_ = EffectType::SPRING.0;
        effect
    }
}

impl From<Friction> for Effect<'_> {
    #[inline]
    fn from(value: Friction) -> Self {
        Self::from([value, value])
    }
}
impl From<[Friction; 2]> for Effect<'_> {
    #[inline]
    fn from([a, b]: [Friction; 2]) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.condition = [a.0.0, b.0.0];
        effect.raw.type_ = EffectType::FRICTION.0;
        effect
    }
}

impl From<Damper> for Effect<'_> {
    #[inline]
    fn from(value: Damper) -> Self {
        Self::from([value, value])
    }
}
impl From<[Damper; 2]> for Effect<'_> {
    #[inline]
    fn from([a, b]: [Damper; 2]) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.condition = [a.0.0, b.0.0];
        effect.raw.type_ = EffectType::DAMPER.0;
        effect
    }
}

impl From<Inertia> for Effect<'_> {
    #[inline]
    fn from(value: Inertia) -> Self {
        Self::from([value, value])
    }
}
impl From<[Inertia; 2]> for Effect<'_> {
    #[inline]
    fn from([a, b]: [Inertia; 2]) -> Self {
        let mut effect = Effect::null();
        effect.raw.u.condition = [a.0.0, b.0.0];
        effect.raw.type_ = EffectType::INERTIA.0;
        effect
    }
}

/// List of supported force-feedback effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EffectKind<'a> {
    Constant(Constant),
    Ramp(Ramp),
    Periodic(Periodic<'a>),
    Rumble(Rumble),
    Spring([Spring; 2]),
    Friction([Friction; 2]),
    Damper([Damper; 2]),
    Inertia([Inertia; 2]),
}

/// A vibration effect.
///
/// Some devices contain two motors of different strengths.
/// The [`Rumble`] effect specifies a *strong* and a *weak* magnitude to support those devices.
///
/// If a device does not have hardware or driver support for rumble effects, but *does*
/// support [`Periodic`] effects, the kernel will attempt to convert the [`Rumble`] effect.
/// This conversion may have undesired effects, so applications may want to try to send a
/// [`Periodic`] effect with [`Waveform::SINE`] if the device supports it, which is not subject to
/// conversion.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Rumble(ff_rumble_effect);

impl Rumble {
    #[inline]
    pub const fn new(strong_magnitude: u16, weak_magnitude: u16) -> Self {
        Self(ff_rumble_effect {
            strong_magnitude,
            weak_magnitude,
        })
    }

    #[inline]
    pub fn strong_magnitude(&self) -> u16 {
        self.0.strong_magnitude
    }

    #[inline]
    pub fn weak_magnitude(&self) -> u16 {
        self.0.weak_magnitude
    }
}
impl fmt::Debug for Rumble {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rumble")
            .field("strong_magnitude", &self.strong_magnitude())
            .field("weak_magnitude", &self.weak_magnitude())
            .finish()
    }
}

/// A periodic waveform effect.
///
/// The `'a` lifetime is only relevant when the effect uses a custom waveform.
/// It indicates the lifetime of that custom waveform data (which the [`Periodic`] struct borrows
/// from).
/// For all other types of [`Periodic`] events, the lifetime can be `'static`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Periodic<'a> {
    raw: ff_periodic_effect,
    _p: PhantomData<&'a ()>,
}

impl<'a> Periodic<'a> {
    /// Creates a simple [`Periodic`] effect with a fixed [`Waveform`].
    #[inline]
    pub fn simple(waveform: Waveform, period: u16, magnitude: i16) -> Periodic<'a> {
        let mut p = Periodic {
            raw: unsafe { mem::zeroed() },
            _p: PhantomData,
        };
        p.raw.waveform = waveform.0;
        p.raw.period = period;
        p.raw.magnitude = magnitude;
        p
    }

    /// Creates a custom waveform effect.
    ///
    /// The device has to advertise support for [`Feature::CUSTOM`] for this to work.
    ///
    /// # Panics
    ///
    /// Panics when `data` is longer than [`u32::MAX`] elements.
    /// In practice, no device will accept a waveform that large anyways.
    #[inline]
    pub fn custom(data: &'a [i16]) -> Periodic<'a> {
        let mut p = Periodic {
            raw: unsafe { mem::zeroed() },
            _p: PhantomData,
        };
        p.raw.waveform = Waveform::CUSTOM.0;
        p.raw.custom_len = data.len().try_into().unwrap();
        p.raw.custom_data = data.as_ptr().cast_mut();
        p
    }

    /// Returns the type of [`Waveform`] described by this [`Periodic`] effect.
    #[inline]
    pub fn waveform(&self) -> Waveform {
        Waveform(self.raw.waveform)
    }

    /// Waveform period in ms.
    #[inline]
    pub fn period(&self) -> u16 {
        self.raw.period
    }

    #[inline]
    pub fn magnitude(&self) -> i16 {
        self.raw.magnitude
    }

    /// Vertical offset of the waveform (and its approximate mean value).
    #[inline]
    pub fn offset(&self) -> i16 {
        self.raw.offset
    }

    /// The horizontal phase offset in the effect's [`Waveform`] where playback will begin.
    #[inline]
    pub fn phase(&self) -> u16 {
        self.raw.phase
    }

    #[inline]
    pub fn envelope(&self) -> Envelope {
        Envelope(self.raw.envelope)
    }

    #[inline]
    pub fn with_envelope(mut self, env: Envelope) -> Self {
        self.raw.envelope = env.0;
        self
    }

    /// If this effect has type [`Waveform::CUSTOM`], returns a reference to the custom waveform
    /// data.
    ///
    /// Also see [`Periodic::custom`] for how to create such an effect.
    #[inline]
    pub fn custom_data(&self) -> Option<&'a [i16]> {
        if self.raw.custom_data.is_null() {
            None
        } else {
            unsafe {
                Some(slice::from_raw_parts(
                    self.raw.custom_data,
                    self.raw.custom_len as usize,
                ))
            }
        }
    }
}

impl PartialEq for Periodic<'_> {
    fn eq(&self, other: &Self) -> bool {
        // This has to be done by hand because the derived impl wouldn't compare the waveform data.
        self.waveform() == other.waveform()
            && self.period() == other.period()
            && self.magnitude() == other.magnitude()
            && self.offset() == other.offset()
            && self.phase() == other.phase()
            && self.envelope() == other.envelope()
            && self.custom_data() == other.custom_data()
    }
}
impl Eq for Periodic<'_> {}

impl fmt::Debug for Periodic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Periodic")
            .field("waveform", &self.waveform())
            .field("period", &self.period())
            .field("magnitude", &self.magnitude())
            .field("offset", &self.offset())
            .field("phase", &self.phase())
            .field("envelope", &self.envelope())
            .field("custom_data", &self.custom_data())
            .finish()
    }
}

/// An effect that applies a constant force.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Constant(ff_constant_effect);

impl Constant {
    #[inline]
    pub fn new(level: i16) -> Self {
        Self(ff_constant_effect {
            level,
            envelope: ff_envelope {
                attack_length: 0,
                attack_level: 0,
                fade_length: 0,
                fade_level: 0,
            },
        })
    }

    #[inline]
    pub fn level(&self) -> i16 {
        self.0.level
    }

    #[inline]
    pub fn envelope(&self) -> Envelope {
        Envelope(self.0.envelope)
    }

    #[inline]
    pub fn with_envelope(mut self, env: Envelope) -> Self {
        self.0.envelope = env.0;
        self
    }
}
impl fmt::Debug for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Constant")
            .field("level", &self.level())
            .field("envelope", &self.envelope())
            .finish()
    }
}

/// An effect that ramps up or down over time.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Ramp(ff_ramp_effect);

impl Ramp {
    #[inline]
    pub fn new(start_level: i16, end_level: i16) -> Self {
        Self(ff_ramp_effect {
            start_level,
            end_level,
            envelope: ff_envelope {
                attack_length: 0,
                attack_level: 0,
                fade_length: 0,
                fade_level: 0,
            },
        })
    }

    #[inline]
    pub fn start_level(&self) -> i16 {
        self.0.start_level
    }

    #[inline]
    pub fn end_level(&self) -> i16 {
        self.0.end_level
    }

    #[inline]
    pub fn envelope(&self) -> Envelope {
        Envelope(self.0.envelope)
    }

    #[inline]
    pub fn with_envelope(mut self, env: Envelope) -> Self {
        self.0.envelope = env.0;
        self
    }
}
impl fmt::Debug for Ramp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ramp")
            .field("start_level", &self.start_level())
            .field("end_level", &self.end_level())
            .field("envelope", &self.envelope())
            .finish()
    }
}

/// An effect that applies conditionally and gradually as an axis is moved.
///
/// Used for [`Spring`], [`Friction`], [`Damper`] and [`Inertia`].
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Condition(ff_condition_effect);

impl Condition {
    /// Creates an empty [`Condition`] that never applies.
    #[inline]
    pub fn new() -> Self {
        Self(ff_condition_effect {
            right_saturation: 0,
            left_saturation: 0,
            right_coeff: 0,
            left_coeff: 0,
            deadband: 0,
            center: 0,
        })
    }

    #[inline]
    pub fn right_saturation(&self) -> u16 {
        self.0.right_saturation
    }

    #[inline]
    pub fn left_saturation(&self) -> u16 {
        self.0.left_saturation
    }

    #[inline]
    pub fn right_coeff(&self) -> i16 {
        self.0.right_coeff
    }

    #[inline]
    pub fn left_coeff(&self) -> i16 {
        self.0.left_coeff
    }

    #[inline]
    pub fn deadband(&self) -> u16 {
        self.0.deadband
    }

    #[inline]
    pub fn center(&self) -> i16 {
        self.0.center
    }

    #[inline]
    pub fn with_right_saturation(mut self, value: u16) -> Self {
        self.0.right_saturation = value;
        self
    }

    #[inline]
    pub fn with_left_saturation(mut self, value: u16) -> Self {
        self.0.left_saturation = value;
        self
    }

    #[inline]
    pub fn with_right_coeff(mut self, value: i16) -> Self {
        self.0.right_coeff = value;
        self
    }

    #[inline]
    pub fn with_left_coeff(mut self, value: i16) -> Self {
        self.0.left_coeff = value;
        self
    }

    #[inline]
    pub fn with_deadband(mut self, value: u16) -> Self {
        self.0.deadband = value;
        self
    }

    #[inline]
    pub fn with_center(mut self, value: i16) -> Self {
        self.0.center = value;
        self
    }
}
impl Default for Condition {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Condition")
            .field("right_saturation", &self.right_saturation())
            .field("left_saturation", &self.left_saturation())
            .field("right_coeff", &self.right_coeff())
            .field("left_coeff", &self.left_coeff())
            .field("deadband", &self.deadband())
            .field("center", &self.center())
            .finish()
    }
}

/// Makes an axis spring back to its center when moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Spring(Condition);

impl Deref for Spring {
    type Target = Condition;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Spring {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl From<Condition> for Spring {
    #[inline]
    fn from(value: Condition) -> Self {
        Self(value)
    }
}
impl From<Spring> for Condition {
    #[inline]
    fn from(value: Spring) -> Self {
        value.0
    }
}

/// Applies friction to an axis as it is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Friction(Condition);

impl Deref for Friction {
    type Target = Condition;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Friction {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl From<Condition> for Friction {
    #[inline]
    fn from(value: Condition) -> Self {
        Self(value)
    }
}
impl From<Friction> for Condition {
    #[inline]
    fn from(value: Friction) -> Self {
        value.0
    }
}

/// Damps movement along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Damper(Condition);

impl Deref for Damper {
    type Target = Condition;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Damper {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl From<Condition> for Damper {
    #[inline]
    fn from(value: Condition) -> Self {
        Self(value)
    }
}
impl From<Damper> for Condition {
    #[inline]
    fn from(value: Damper) -> Self {
        value.0
    }
}

/// Resists changes of velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Inertia(Condition);

impl Deref for Inertia {
    type Target = Condition;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Inertia {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl From<Condition> for Inertia {
    #[inline]
    fn from(value: Condition) -> Self {
        Self(value)
    }
}
impl From<Inertia> for Condition {
    #[inline]
    fn from(value: Inertia) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync>() {}

        assert::<Effect<'static>>();
        assert::<EffectKind<'static>>();
        assert::<EffectType>();
        assert::<EffectId>();
    }

    #[test]
    fn custom_eq() {
        // These `Periodic` effects point to different waveforms, but those waveforms are equal.
        static BUF: &[i16] = &[0, 1, 2, 3, 2, 1, 0];
        let a = Periodic::custom(&BUF[0..1]); // &[0]
        let b = Periodic::custom(&BUF[6..7]); // &[0]

        assert_eq!(a, b);
    }
}
