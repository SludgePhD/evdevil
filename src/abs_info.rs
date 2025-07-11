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
    pub const fn new(minimum: i32, maximum: i32) -> Self {
        Self(input_absinfo {
            minimum,
            maximum,
            ..unsafe { mem::zeroed() }
        })
    }

    /// Returns the axis' current value, clamped to the valid range.
    pub fn value(&self) -> i32 {
        let [min, max] = [self.minimum(), self.maximum()];
        let [min, max] = if min <= max { [min, max] } else { [max, min] };
        self.raw_value().clamp(min, max)
    }

    /// Returns the raw value of the axis, without clamping.
    ///
    /// This is *typically* between [`AbsInfo::minimum`] and [`AbsInfo::maximum`], but this is not
    /// enforced by the kernel. [`AbsInfo::value`] clamps the value to the valid range.
    pub const fn raw_value(&self) -> i32 {
        self.0.value
    }

    pub const fn with_raw_value(mut self, value: i32) -> Self {
        self.0.value = value;
        self
    }

    pub const fn minimum(&self) -> i32 {
        self.0.minimum
    }

    pub const fn with_minimum(mut self, minimum: i32) -> Self {
        self.0.minimum = minimum;
        self
    }

    pub const fn maximum(&self) -> i32 {
        self.0.maximum
    }

    pub const fn with_maximum(mut self, maximum: i32) -> Self {
        self.0.maximum = maximum;
        self
    }

    pub const fn fuzz(&self) -> i32 {
        self.0.fuzz
    }

    pub const fn with_fuzz(mut self, fuzz: i32) -> Self {
        self.0.fuzz = fuzz;
        self
    }

    pub const fn flat(&self) -> i32 {
        self.0.flat
    }

    pub const fn with_flat(mut self, flat: i32) -> Self {
        self.0.flat = flat;
        self
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
    pub const fn resolution(&self) -> i32 {
        self.0.resolution
    }

    pub const fn with_resolution(mut self, resolution: i32) -> Self {
        self.0.resolution = resolution;
        self
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
