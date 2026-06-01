use std::collections::HashMap;
use std::{fmt::Display, ops::Deref};

use crate::error::LexDataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Word<const N: usize>([char; N]);

impl<const N: usize> TryFrom<&str> for Word<N> {
    type Error = LexDataError;

    /// Zero heap allocation: chars are written directly into a stack-allocated [char; N].
    ///
    /// TODO: add TryFrom<&[u8]> ASCII fast path for use with memmap2 in load.rs:
    /// widen each byte directly to char into the stack array — zero heap allocations.
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let mut array = ['\0'; N];
        let mut i = 0;
        for ch in s.chars() {
            if i >= N {
                return Err(LexDataError::WordLengthError {
                    expected: N,
                    got: i + 1,
                });
            }
            array[i] = ch;
            i += 1;
        }
        if i != N {
            return Err(LexDataError::WordLengthError {
                expected: N,
                got: i,
            });
        }
        Ok(Word(array))
    }
}

impl<const N: usize> Deref for Word<N> {
    type Target = [char; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> Display for Word<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in &self.0 {
            write!(f, "{}", ch)?;
        }
        Ok(())
    }
}

impl<const N: usize> IntoIterator for Word<N> {
    type Item = char;
    type IntoIter = std::array::IntoIter<char, N>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterator::into_iter(*self)
    }
}

pub struct WordSet<const N: usize> {
    /// Raw frequency counts, for probability normalization.
    pub frequencies: HashMap<Word<N>, u64>,
}

impl<const N: usize> WordSet<N> {
    /// Words sorted descending by frequency (same order as the on-disk CSV).
    pub fn words(&self) -> Vec<Word<N>> {
        let mut words: Vec<Word<N>> = self.frequencies.keys().copied().collect();
        words.sort_by_key(|w| std::cmp::Reverse(self.frequencies[w]));
        words
    }

    /// Probabilities normalized to sum to 1.0, for use in the Guesser.
    pub fn probabilities(&self) -> HashMap<Word<N>, f64> {
        let total: f64 = self.frequencies.values().copied().map(|f| f as f64).sum();
        self.frequencies
            .iter()
            .map(|(&word, &freq)| (word, freq as f64 / total))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.frequencies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frequencies.is_empty()
    }

    pub fn limit(&mut self, n: usize) {
        let mut words = self.words();
        words.truncate(n);
        self.frequencies.retain(|word, _| words.contains(word));
    }
}
