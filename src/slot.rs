use std::num::TryFromIntError;

/// A multitouch slot index.
///
/// A list of [`Slot`]s with valid data can be retrieved via [`EventReader::valid_slots`].
///
/// [`EventReader::valid_slots`]: crate::EventReader::valid_slots
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot(i32);

// The slot index is sent as an event value, which has type i32.
// `Slot` adds the invariant that the value is always >= 0.

impl Slot {
    pub(crate) fn raw(self) -> i32 {
        self.0
    }

    pub(crate) fn from_raw(raw: i32) -> Self {
        Self(raw)
    }
}

impl From<u16> for Slot {
    #[inline]
    fn from(value: u16) -> Self {
        Self(value.into())
    }
}

impl From<u8> for Slot {
    #[inline]
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

impl TryFrom<i32> for Slot {
    type Error = TryFromIntError;

    #[inline]
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        // This checks that it isn't negative:
        let nonneg = u32::try_from(value)? as i32;
        Ok(Self(nonneg))
    }
}

impl PartialEq<i32> for Slot {
    fn eq(&self, other: &i32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<u16> for Slot {
    fn eq(&self, other: &u16) -> bool {
        *self == Slot::from(*other)
    }
}

impl PartialEq<u8> for Slot {
    fn eq(&self, other: &u8) -> bool {
        *self == Slot::from(*other)
    }
}
