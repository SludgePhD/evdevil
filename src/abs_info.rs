use std::{fmt, mem};

use crate::raw::input::input_absinfo;

#[expect(unused_imports)] // docs only
use crate::event::Abs;

/// Information about an absolute axis ([`Abs`]).
///
/// Contains the axis' current value, as well as range and resolution information.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct AbsInfo(pub(crate) input_absinfo);

impl AbsInfo {
    /// Creates a new [`AbsInfo`] with a minimum and maximum value.
    ///
    /// All other fields start out as zero.
    #[inline]
    pub const fn new(minimum: i32, maximum: i32) -> Self {
        Self(input_absinfo {
            minimum,
            maximum,
            ..unsafe { mem::zeroed() }
        })
    }

    /// Returns a copy of `self` with the given axis value.
    ///
    /// The value is not clamped to the minimum/maximum or modified in any other way.
    #[inline]
    pub const fn with_raw_value(mut self, value: i32) -> Self {
        self.0.value = value;
        self
    }

    /// Returns a copy of `self` with the given minimum.
    #[inline]
    pub const fn with_minimum(mut self, minimum: i32) -> Self {
        self.0.minimum = minimum;
        self
    }

    /// Returns a copy of `self` with the given maximum.
    #[inline]
    pub const fn with_maximum(mut self, maximum: i32) -> Self {
        self.0.maximum = maximum;
        self
    }

    /// Returns a copy of `self` with the given fuzz value.
    #[inline]
    pub const fn with_fuzz(mut self, fuzz: i32) -> Self {
        self.0.fuzz = fuzz;
        self
    }

    /// Returns a copy of `self` with the given flat value.
    #[inline]
    pub const fn with_flat(mut self, flat: i32) -> Self {
        self.0.flat = flat;
        self
    }

    /// Returns a copy of `self` with the given axis resolution.
    ///
    /// See [`AbsInfo::resolution`] for more information.
    #[inline]
    pub const fn with_resolution(mut self, resolution: i32) -> Self {
        self.0.resolution = resolution;
        self
    }

    /// Returns the axis' current value, clamped to the valid range.
    #[inline]
    pub fn value(&self) -> i32 {
        let [min, max] = [self.minimum(), self.maximum()];
        let [min, max] = if min <= max { [min, max] } else { [max, min] };
        self.raw_value().clamp(min, max)
    }

    /// Returns the raw value of the axis, without clamping.
    ///
    /// This is *typically* between [`AbsInfo::minimum`] and [`AbsInfo::maximum`], but this is not
    /// enforced by the kernel. [`AbsInfo::value`] clamps the value to the valid range.
    #[inline]
    pub const fn raw_value(&self) -> i32 {
        self.0.value
    }

    /// Returns the minimum value of this axis.
    #[inline]
    pub const fn minimum(&self) -> i32 {
        self.0.minimum
    }

    /// Returns the minimum value of this axis.
    #[inline]
    pub const fn maximum(&self) -> i32 {
        self.0.maximum
    }

    /// Returns the *fuzz* value of the axis.
    ///
    /// The *fuzz* value is used by the kernel to filter out noise.
    #[inline]
    pub const fn fuzz(&self) -> i32 {
        self.0.fuzz
    }

    /// Returns the *flat* value of the axis.
    ///
    /// The *flat* value configures the axis deadzone.
    #[inline]
    pub const fn flat(&self) -> i32 {
        self.0.flat
    }

    /// Returns the resolution of this axis.
    ///
    /// This is *not* the granularity of the value field (which is always 1), but the relation of
    /// the axis value to physical units.
    ///
    /// The resolution for the main axes ([`Abs::X`], [`Abs::Y`], [`Abs::Z`], and
    /// [`Abs::MT_POSITION_X`] and [`Abs::MT_POSITION_Y`]) is typically specified in
    /// **units/mm**.
    ///
    /// If the device has set [`InputProp::ACCELEROMETER`][crate::InputProp::ACCELEROMETER], the
    /// units for the main X/Y/Z axes are in **units/g** instead.
    ///
    /// Rotational axes ([`Abs::RX`], [`Abs::RY`], [`Abs::RZ`]) use **units/radian**.
    ///
    /// **Note**: This value is commonly reported incorrectly, so device-specific overrides might
    /// be needed.
    #[inline]
    pub const fn resolution(&self) -> i32 {
        self.0.resolution
    }
}

impl fmt::Debug for AbsInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbsInfo")
            .field("value", &self.raw_value())
            .field("minimum", &self.minimum())
            .field("maximum", &self.maximum())
            .field("fuzz", &self.fuzz())
            .field("flat", &self.flat())
            .field("resolution", &self.resolution())
            .finish()
    }
}
