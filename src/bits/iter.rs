//! A generic iterator implementation for bit sets.
//!
//! Can be instantiated with any [`NextWord`] implementation, which can yield words from a single
//! [`BitSet`][super::BitSet], or words from combining elements from multiple sets.

use std::marker::PhantomData;

use super::{BitValue, Word};

pub struct IterImpl<V: BitValue, N> {
    next_word: N,
    word: Word,
    bits_left: u32,    // bits left in `word`
    next_index: usize, // element index of the lowest bit in `word`
    _p: PhantomData<V>,
}

impl<V: BitValue, N: Clone> Clone for IterImpl<V, N> {
    fn clone(&self) -> Self {
        Self {
            next_word: self.next_word.clone(),
            word: self.word,
            bits_left: self.bits_left,
            next_index: self.next_index,
            _p: PhantomData,
        }
    }
}
impl<V: BitValue, N: Copy> Copy for IterImpl<V, N> {}

impl<V: BitValue, N> IterImpl<V, N> {
    pub fn new(next_word: N) -> Self {
        Self {
            next_word,
            word: 0,
            bits_left: 0,
            next_index: 0,
            _p: PhantomData,
        }
    }
}

impl<V: BitValue, N: Iterator<Item = Word>> Iterator for IterImpl<V, N> {
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        if self.word == 0 {
            self.next_index += self.bits_left as usize;
            self.bits_left = 0;
        }
        if self.bits_left == 0 {
            // Refill `self.word` with a word that contains at least one set bit.
            loop {
                self.word = self.next_word.next()?;

                if self.word == 0 {
                    self.next_index += Word::BITS as usize;
                } else {
                    break;
                }
            }
            self.bits_left = Word::BITS;
        }

        // Get the position of the next 1 in the word.
        // Since we never put an all-zeroes word in there, there must be one.
        let zeroes = self.word.trailing_zeros();
        debug_assert_ne!(zeroes, Word::BITS);

        // Index in the `BitSet` of the bit we found.
        let index = self.next_index + zeroes as usize;

        // Shift the 1 and all preceding zeroes out of the word and update counts accordingly.
        self.word >>= zeroes as usize;
        self.word >>= 1;

        self.next_index += zeroes as usize + 1;
        self.bits_left -= zeroes + 1;

        Some(V::from_index(index))
    }
}
