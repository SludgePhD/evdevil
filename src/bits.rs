//! A [`BitSet`] for values reported by `evdev`.
//!
//! `evdev` reports device properties, present axes, and pressed keys as bit sets. The types in this
//! module are meant to represent that data with a convenient API.

mod iter;

use sealed::Array;
pub(crate) use sealed::BitValueImpl;

use std::{ffi::c_ulong, fmt, slice};

mod sealed {
    use super::Word;

    pub trait BitValueImpl {
        #[doc(hidden)]
        type __PrivateArray: AsRef<[Word]>
            + AsMut<[Word]>
            + Copy
            + IntoIterator<Item = Word, IntoIter: Clone>;
        #[doc(hidden)]
        const __PRIVATE_ZERO: Self::__PrivateArray;
        // These are all internal semver-exempt implementation details.

        // `index` must fit in the native integer type
        fn from_index(index: usize) -> Self;
        fn into_index(self) -> usize;
    }

    pub(crate) type Array<V> = <V as BitValueImpl>::__PrivateArray;
}

/// The underlying word type used by [`BitSet`]s.
///
/// This is an `unsigned long` in C, which may vary between platforms.
pub type Word = c_ulong;

/// Types that can be used in [`BitSet`].
///
/// This is a sealed trait with no interface. It is implemented for types in this library that
/// `evdev` reports to userspace using bitfields.
pub trait BitValue: Copy + sealed::BitValueImpl {
    /// The largest value that can be stored in a [`BitSet`].
    ///
    /// Attempting to insert a value above this into a [`BitSet`] will panic.
    ///
    /// Note that the exact value used for this associated constant should not be relied on as it
    /// is not stable.
    /// It may be incremented in any minor or patch release to update the underlying enumeration
    /// with upstream additions.
    const MAX: Self;
}

/// A set of `V`, stored as a bit set.
pub struct BitSet<V: BitValue> {
    pub(crate) words: Array<V>,
}

impl<V: BitValue> Copy for BitSet<V> {}
impl<V: BitValue> Clone for BitSet<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V: BitValue> Default for BitSet<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: BitValue> BitSet<V> {
    /// Creates an empty bit set that doesn't contain any values.
    pub const fn new() -> Self {
        Self {
            words: V::__PRIVATE_ZERO,
        }
    }

    /// Returns a reference to the underlying [`Word`]s making up this [`BitSet`].
    ///
    /// Note that the [`Word`] type varies in size and endianness between platforms, so if you want
    /// to use this for cross-platform serialization, make sure to convert the data to something
    /// portable first.
    ///
    /// The number of [`Word`]s that make up any given [`BitSet`] can also vary between platforms,
    /// and is generally only guaranteed to be large enough to store
    /// [`<V as BitValue>::MAX`][BitValue::MAX], but may be arbitrarily larger.
    pub fn words(&self) -> &[Word] {
        self.words.as_ref()
    }

    /// Returns a mutable reference to the underlying [`Word`]s making up this [`BitSet`].
    ///
    /// You should not set any bits to 1 whose indices are larger than
    /// [`<V as BitValue>::MAX`][BitValue::MAX]. Doing so might cause the [`BitSet`] to behave
    /// incorrectly.
    ///
    /// Note that the [`Word`] type varies in size between platforms.
    pub fn words_mut(&mut self) -> &mut [Word] {
        self.words.as_mut()
    }

    /// Returns the number of elements in this [`BitSet`] (the number of set bits).
    pub fn len(&self) -> usize {
        self.words
            .as_ref()
            .iter()
            .map(|w| w.count_ones() as usize)
            .sum::<usize>()
    }

    /// Returns whether this [`BitSet`] is empty (contains no set bits).
    pub fn is_empty(&self) -> bool {
        self.words.as_ref().iter().all(|&w| w == 0)
    }

    /// Returns whether `self` contains `value`.
    pub fn contains(&self, value: V) -> bool {
        if value.into_index() > V::MAX.into_index() {
            return false;
        }
        let index = value.into_index();
        let wordpos = index / Word::BITS as usize;
        let bitpos = index % Word::BITS as usize;

        let word = self.words.as_ref()[wordpos];
        let bit = word & (1 << bitpos) != 0;
        bit
    }

    /// Inserts `value` into `self`, setting the appropriate bit.
    ///
    /// Returns `true` if `value` was newly inserted, or `false` if it was already present.
    ///
    /// # Panics
    ///
    /// Panics if `value` is larger than [`<V as BitValue>::MAX`][BitValue::MAX].
    pub fn insert(&mut self, value: V) -> bool {
        assert!(
            value.into_index() <= V::MAX.into_index(),
            "value out of range for `BitSet` storage (value's index is {}, max is {})",
            value.into_index(),
            V::MAX.into_index(),
        );

        let present = self.contains(value);

        let index = value.into_index();
        let wordpos = index / Word::BITS as usize;
        let bitpos = index % Word::BITS as usize;
        self.words.as_mut()[wordpos] |= 1 << bitpos;
        present
    }

    /// Removes `value` from the set.
    ///
    /// Returns `true` if it was present and has been removed, or `false` if it was not present.
    pub fn remove(&mut self, value: V) -> bool {
        if value.into_index() > V::MAX.into_index() {
            return false;
        }
        let present = self.contains(value);

        let index = value.into_index();
        let wordpos = index / Word::BITS as usize;
        let bitpos = index % Word::BITS as usize;
        self.words.as_mut()[wordpos] &= !(1 << bitpos);
        present
    }

    /// Returns an iterator over all values in `self`.
    pub fn iter(&self) -> Iter<'_, V> {
        Iter {
            imp: iter::IterImpl::new(self.words.as_ref().iter().copied()),
        }
    }

    /// Returns an iterator over all values that are contained in either `self` or `other`, but not
    /// both.
    pub(crate) fn symmetric_difference<'a>(
        &'a self,
        other: &'a BitSet<V>,
    ) -> SymmetricDifference<'a, V> {
        SymmetricDifference {
            imp: iter::IterImpl::new(SymmDiffWords {
                a: self.words.as_ref().iter(),
                b: other.words.as_ref().iter(),
            }),
        }
    }
}

impl<V: BitValue + fmt::Debug> fmt::Debug for BitSet<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<V: BitValue> PartialEq for BitSet<V> {
    fn eq(&self, other: &Self) -> bool {
        self.words.as_ref() == other.words.as_ref()
    }
}
impl<V: BitValue> Eq for BitSet<V> {}

impl<V: BitValue> FromIterator<V> for BitSet<V> {
    fn from_iter<T: IntoIterator<Item = V>>(iter: T) -> Self {
        let mut this = Self::new();
        this.extend(iter);
        this
    }
}
impl<V: BitValue> Extend<V> for BitSet<V> {
    fn extend<T: IntoIterator<Item = V>>(&mut self, iter: T) {
        for item in iter {
            self.insert(item);
        }
    }
}

impl<'a, V: BitValue> IntoIterator for &'a BitSet<V> {
    type Item = V;
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<V: BitValue> IntoIterator for BitSet<V> {
    type Item = V;
    type IntoIter = IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            imp: iter::IterImpl::new(self.words.into_iter()),
        }
    }
}

/// An owning iterator over the values stored in a [`BitSet`].
pub struct IntoIter<V: BitValue> {
    imp: iter::IterImpl<V, <Array<V> as IntoIterator>::IntoIter>,
}
impl<V: BitValue> Iterator for IntoIter<V> {
    type Item = V;
    fn next(&mut self) -> Option<Self::Item> {
        self.imp.next()
    }
}
impl<V: BitValue + fmt::Debug> fmt::Debug for IntoIter<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IntoIter")
            .field(&DebugAsSet(self.imp.clone()))
            .finish()
    }
}

/// An iterator over the values stored in a [`BitSet`].
pub struct Iter<'a, V: BitValue> {
    imp: iter::IterImpl<V, std::iter::Copied<slice::Iter<'a, Word>>>,
}

impl<V: BitValue> Iterator for Iter<'_, V> {
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        self.imp.next()
    }
}

impl<V: BitValue + fmt::Debug> fmt::Debug for Iter<'_, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Iter")
            .field(&DebugAsSet(self.imp.clone()))
            .finish()
    }
}

struct DebugAsSet<I>(I);
impl<I: Clone + Iterator> fmt::Debug for DebugAsSet<I>
where
    I::Item: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.clone()).finish()
    }
}

/// An [`Iterator`] yielding elements that are in exactly one of two [`BitSet`]s.
///
/// Returned by [`BitSet::symmetric_difference`].
pub(crate) struct SymmetricDifference<'a, V: BitValue> {
    imp: iter::IterImpl<V, SymmDiffWords<'a>>,
}

struct SymmDiffWords<'a> {
    a: slice::Iter<'a, Word>,
    b: slice::Iter<'a, Word>,
}

impl Iterator for SymmDiffWords<'_> {
    type Item = Word;
    fn next(&mut self) -> Option<Word> {
        Some(self.a.next().copied()? ^ self.b.next().copied()?)
    }
}

impl<V: BitValue> Iterator for SymmetricDifference<'_, V> {
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        self.imp.next()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        InputProp,
        event::{Abs, EventType, Key, Led, Misc, Rel},
    };

    use super::*;

    #[test]
    fn sizes() {
        // `BitSet`s are only as big as needed.
        assert_eq!(size_of::<BitSet<EventType>>(), size_of::<Word>());
        assert_eq!(size_of::<BitSet<InputProp>>(), size_of::<Word>());
        assert_eq!(size_of::<BitSet<Rel>>(), size_of::<Word>());
        assert_eq!(size_of::<BitSet<Misc>>(), size_of::<Word>());
        assert_eq!(size_of::<BitSet<Led>>(), size_of::<Word>());
    }

    #[test]
    fn bit0() {
        let mut set = BitSet::new();
        set.insert(InputProp(0));

        assert!(set.contains(InputProp::POINTER));
        assert!(!set.contains(InputProp::DIRECT));
        assert!(!set.contains(InputProp::MAX));
        assert!(!set.contains(InputProp(InputProp::MAX.0 + 1)));
        assert!(!set.contains(InputProp(u8::MAX)));

        assert_eq!(set.iter().collect::<Vec<_>>(), &[InputProp::POINTER]);
    }

    #[test]
    fn max() {
        let mut set = BitSet::new();
        set.insert(InputProp::MAX);

        assert!(!set.contains(InputProp::POINTER));
        assert!(!set.contains(InputProp::DIRECT));
        assert!(set.contains(InputProp::MAX));
        assert!(!set.contains(InputProp(InputProp::MAX.0 + 1)));
        assert!(!set.remove(InputProp(InputProp::MAX.0 + 1)));
    }

    #[test]
    #[should_panic = "value out of range for `BitSet`"]
    fn above_max() {
        let mut set = BitSet::new();
        set.insert(Abs::from_raw(Abs::MAX.raw() + 1));
    }

    #[test]
    fn debug() {
        let set = BitSet::from_iter([Abs::X, Abs::Y, Abs::BRAKE]);
        assert_eq!(format!("{set:?}"), "{ABS_X, ABS_Y, ABS_BRAKE}");

        let mut iter = set.iter();
        assert_eq!(iter.next(), Some(Abs::X));
        assert_eq!(format!("{iter:?}"), "Iter({ABS_Y, ABS_BRAKE})");

        let mut iter = set.into_iter();
        assert_eq!(iter.next(), Some(Abs::X));
        assert_eq!(format!("{iter:?}"), "IntoIter({ABS_Y, ABS_BRAKE})");
    }

    #[test]
    fn multiple() {
        let mut set = BitSet::new();
        set.insert(Key::KEY_RESERVED);
        set.insert(Key::KEY_Q);
        set.insert(Key::MAX);
        set.insert(Key::KEY_MACRO1);

        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            &[Key::KEY_RESERVED, Key::KEY_Q, Key::KEY_MACRO1, Key::MAX]
        );
    }

    #[test]
    fn symmdiff() {
        let mut a = BitSet::new();
        a.insert(Key::KEY_B);

        assert_eq!(
            a.symmetric_difference(&BitSet::new()).collect::<Vec<_>>(),
            &[Key::KEY_B]
        );
        assert_eq!(
            BitSet::new().symmetric_difference(&a).collect::<Vec<_>>(),
            &[Key::KEY_B]
        );

        let mut b = BitSet::new();
        b.insert(Key::KEY_A);

        assert_eq!(
            a.symmetric_difference(&b).collect::<Vec<_>>(),
            &[Key::KEY_A, Key::KEY_B]
        );
        assert_eq!(
            b.symmetric_difference(&a).collect::<Vec<_>>(),
            &[Key::KEY_A, Key::KEY_B]
        );

        assert_eq!(a.symmetric_difference(&a).collect::<Vec<_>>(), &[]);
        assert_eq!(
            BitSet::<Key>::new()
                .symmetric_difference(&BitSet::new())
                .collect::<Vec<_>>(),
            &[]
        );
    }
}
