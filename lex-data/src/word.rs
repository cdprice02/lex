use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Deref;
use std::sync::Arc;

use memmap2::{Mmap, MmapMut};

use crate::error::LexDataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Word<const N: usize>([char; N]);

impl<const N: usize> From<[char; N]> for Word<N> {
    fn from(chars: [char; N]) -> Self {
        Self(chars)
    }
}

impl<const N: usize> TryFrom<&str> for Word<N> {
    type Error = LexDataError;

    /// Zero heap allocation: chars are written directly into a stack-allocated [char; N].
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let chars_count = s.chars().count();
        if chars_count != N {
            return Err(LexDataError::WordLengthError {
                expected: N,
                got: chars_count,
            });
        }
        let mut chars = s.chars();
        Ok(Word::from(std::array::from_fn(|_| {
            chars.next().expect("N chars as checked above")
        })))
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
        self.0.into_iter()
    }
}

/// Zero-copy view of one binary ngram record: `N` char scalars stored as `u32` (LE),
/// followed by a `u64` frequency (LE). Total: `N * 4 + 8` bytes.
pub(crate) struct RawRecord<'a, const N: usize>(&'a [u8]);

impl<const N: usize> RawRecord<'_, N> {
    pub(crate) const CHAR_BYTES: usize = std::mem::size_of::<u32>();
    pub(crate) const FREQ_BYTES: usize = std::mem::size_of::<u64>();
    pub(crate) const SIZE: usize = Self::CHAR_BYTES * N + Self::FREQ_BYTES;

    /// Returns `None` if any stored scalar is not a valid Unicode scalar value.
    pub(crate) fn word(&self) -> Option<Word<N>> {
        let mut chars = ['\0'; N];
        for (i, char_slot) in chars.iter_mut().enumerate() {
            let off = i * Self::CHAR_BYTES;
            let scalar =
                u32::from_le_bytes(self.0[off..off + Self::CHAR_BYTES].try_into().unwrap());
            *char_slot = char::from_u32(scalar)?;
        }
        Some(Word::from(chars))
    }

    pub(crate) fn freq(&self) -> u64 {
        let off = Self::CHAR_BYTES * N;
        u64::from_le_bytes(self.0[off..off + Self::FREQ_BYTES].try_into().unwrap())
    }
}

impl<'a, const N: usize> From<&'a [u8]> for RawRecord<'a, N> {
    fn from(bytes: &'a [u8]) -> Self {
        debug_assert_eq!(bytes.len(), Self::SIZE);
        RawRecord(bytes)
    }
}

/// Iterator over the active words in a [`WordSet`], reading lazily from the mmap backing store.
///
/// Implements [`Clone`] to support `.cycle()`.
pub struct WordIter<'a, const N: usize> {
    mmap: &'a Mmap,
    offsets: &'a [u32],
    index: usize,
}

impl<const N: usize> Iterator for WordIter<'_, N> {
    type Item = Word<N>;

    fn next(&mut self) -> Option<Word<N>> {
        loop {
            let &off = self.offsets.get(self.index)?;
            self.index += 1;
            if let Some(word) =
                RawRecord::<N>::from(&self.mmap[off as usize..off as usize + RawRecord::<N>::SIZE])
                    .word()
            {
                return Some(word);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.offsets.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<const N: usize> Clone for WordIter<'_, N> {
    fn clone(&self) -> Self {
        Self {
            mmap: self.mmap,
            offsets: self.offsets,
            index: self.index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WordSet<const N: usize> {
    mmap: Arc<Mmap>,
    // Byte offsets of active records within the mmap, sorted descending by frequency.
    // Elements are only ever removed (via retain/limit) — never reordered after construction.
    offsets: Vec<u32>,
    // Temperature-scaled probabilities, parallel to offsets. Recomputed after every mutation.
    scaled_probs: Vec<f64>,
}

impl<const N: usize> WordSet<N> {
    /// Constructs a `WordSet` from a memory-mapped binary ngrams file.
    ///
    /// Records must be sorted descending by frequency. `num_records` selects how many
    /// records to expose (use `mmap.len() / RawRecord::<N>::SIZE` for all of them).
    pub(crate) fn from_mmap(mmap: Mmap, num_records: usize) -> Self {
        let mmap = Arc::new(mmap);
        let offsets: Vec<u32> = (0..num_records)
            .map(|i| (i * RawRecord::<N>::SIZE) as u32)
            .collect();
        let freqs = Self::read_freqs(&mmap, &offsets);
        let scaled_probs = Self::compute_probs(&freqs);
        Self {
            mmap,
            offsets,
            scaled_probs,
        }
    }

    /// Constructs a `WordSet` from a word-to-frequency map.
    ///
    /// Sorts by frequency descending and writes records into an anonymous memory map.
    pub fn from_frequency_map(frequencies: HashMap<Word<N>, u64>) -> Self {
        let mut pairs: Vec<(Word<N>, u64)> = frequencies.into_iter().collect();
        pairs.sort_by_key(|&(_, f)| std::cmp::Reverse(f));
        let record_size = RawRecord::<N>::SIZE;
        let total = pairs.len() * record_size;
        // map_anon(0) fails on many platforms; allocate at least 1 byte.
        let mut buf = MmapMut::map_anon(total.max(1)).expect("anonymous mmap allocation failed");
        for (i, (word, freq)) in pairs.iter().enumerate() {
            let off = i * record_size;
            for (j, &ch) in word.iter().enumerate() {
                let char_off = off + j * RawRecord::<N>::CHAR_BYTES;
                buf[char_off..char_off + RawRecord::<N>::CHAR_BYTES]
                    .copy_from_slice(&(ch as u32).to_le_bytes());
            }
            let freq_off = off + RawRecord::<N>::CHAR_BYTES * N;
            buf[freq_off..freq_off + RawRecord::<N>::FREQ_BYTES]
                .copy_from_slice(&freq.to_le_bytes());
        }
        let mmap = buf.make_read_only().expect("failed to make mmap read-only");
        Self::from_mmap(mmap, pairs.len())
    }

    /// Temperature-scaled softmax over log-frequencies: p_i ∝ f_i^(1/T).
    /// Raw-frequency softmax collapses to one-hot on the top word because Ngrams
    /// counts span many orders of magnitude; log-space input + temperature > 1
    /// flattens the distribution toward uniform without discarding rank information.
    fn compute_probs(frequencies: &[u64]) -> Vec<f64> {
        // T=1 → linear normalization; T→∞ → uniform; tuned empirically
        const TEMPERATURE: f64 = 5.0;
        if frequencies.is_empty() {
            return Vec::new();
        }
        let log_freqs: Vec<f64> = frequencies.iter().map(|&f| (f as f64).ln()).collect();
        let max_lf = log_freqs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exp: Vec<f64> = log_freqs
            .iter()
            .map(|&lf| ((lf - max_lf) / TEMPERATURE).exp())
            .collect();
        let sum: f64 = exp.iter().sum();
        exp.into_iter().map(|e| e / sum).collect()
    }

    fn read_freqs(mmap: &Mmap, offsets: &[u32]) -> Vec<u64> {
        offsets
            .iter()
            .map(|&off| {
                RawRecord::<N>::from(&mmap[off as usize..off as usize + RawRecord::<N>::SIZE])
                    .freq()
            })
            .collect()
    }

    /// Words sorted descending by frequency, read lazily from the mmap backing store.
    pub fn words(&self) -> WordIter<'_, N> {
        WordIter {
            mmap: &self.mmap,
            offsets: &self.offsets,
            index: 0,
        }
    }

    /// Temperature-scaled probabilities (p_i ∝ f_i^(1/T)), parallel-indexed with `words()`.
    pub fn probabilities(&self) -> &[f64] {
        &self.scaled_probs
    }

    /// Iterator of `(word, probability)` pairs in frequency-descending order.
    pub fn word_probs(&self) -> impl Iterator<Item = (Word<N>, f64)> + '_ {
        self.words().zip(self.scaled_probs.iter().copied())
    }

    pub fn frequency(&self, word: &Word<N>) -> Option<u64> {
        for &off in &self.offsets {
            let record =
                RawRecord::<N>::from(&self.mmap[off as usize..off as usize + RawRecord::<N>::SIZE]);
            if record.word().as_ref() == Some(word) {
                return Some(record.freq());
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    pub fn contains(&self, word: &Word<N>) -> bool {
        self.frequency(word).is_some()
    }

    pub fn limit(&mut self, n: usize) {
        if n < self.offsets.len() {
            self.offsets.truncate(n);
            let freqs = Self::read_freqs(&self.mmap, &self.offsets);
            self.scaled_probs = Self::compute_probs(&freqs);
        }
    }

    pub fn retain<F: Fn(&Word<N>) -> bool>(&mut self, predicate: F) {
        let mut new_offsets = Vec::new();
        let mut new_freqs = Vec::new();
        for &off in &self.offsets {
            let record =
                RawRecord::<N>::from(&self.mmap[off as usize..off as usize + RawRecord::<N>::SIZE]);
            if record.word().is_some_and(|w| predicate(&w)) {
                new_offsets.push(off);
                new_freqs.push(record.freq());
            }
        }
        self.offsets = new_offsets;
        self.scaled_probs = Self::compute_probs(&new_freqs);
    }
}

#[cfg(test)]
mod benches {
    extern crate test;
    use std::collections::HashMap;
    use std::hint::black_box;
    use test::Bencher;

    use super::*;

    fn make_wordset(n: usize) -> WordSet<5> {
        let frequencies = (0..n)
            .map(|i| {
                let s: String = [
                    char::from(b'a' + (i % 26) as u8),
                    char::from(b'a' + ((i / 26) % 26) as u8),
                    char::from(b'a' + ((i / 676) % 26) as u8),
                    char::from(b'a' + ((i / 17576) % 26) as u8),
                    char::from(b'a' + ((i / 456976) % 26) as u8),
                ]
                .iter()
                .collect();
                (Word::<5>::try_from(s.as_str()).unwrap(), (n - i) as u64)
            })
            .collect::<HashMap<Word<5>, u64>>();
        WordSet::from_frequency_map(frequencies)
    }

    #[bench]
    fn wordset_probabilities_1000(b: &mut Bencher) {
        let ws = make_wordset(1000);
        b.iter(|| black_box(ws.probabilities()));
    }

    #[bench]
    fn wordset_limit_1000_to_100(b: &mut Bencher) {
        b.iter(|| {
            let mut ws = make_wordset(1000);
            ws.limit(100);
            black_box(ws.len())
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn ws(pairs: &[(&str, u64)]) -> WordSet<5> {
        let frequencies: HashMap<Word<5>, u64> = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        WordSet::from_frequency_map(frequencies)
    }

    #[test]
    fn try_from_exact() {
        assert!(Word::<5>::try_from("crane").is_ok());
    }

    #[test]
    fn try_from_too_short() {
        let err = Word::<5>::try_from("four").unwrap_err();
        assert!(matches!(
            err,
            crate::error::LexDataError::WordLengthError {
                expected: 5,
                got: 4
            }
        ));
    }

    #[test]
    fn try_from_too_long() {
        assert!(Word::<5>::try_from("toolong").is_err());
    }

    #[test]
    fn try_from_empty() {
        let err = Word::<5>::try_from("").unwrap_err();
        assert!(matches!(
            err,
            crate::error::LexDataError::WordLengthError {
                expected: 5,
                got: 0
            }
        ));
    }

    #[test]
    fn try_from_multibyte_french() {
        // "café" = c, a, f, é (4 Unicode scalars in NFC) → Word::<4>
        assert!(Word::<4>::try_from("café").is_ok());
    }

    #[test]
    fn try_from_german_umlaut() {
        // "größe" = g, r, ö, ß, e (5 NFC scalars)
        assert!(Word::<5>::try_from("größe").is_ok());
    }

    #[test]
    fn try_from_cyrillic() {
        // "слово" = 5 Cyrillic scalars
        assert!(Word::<5>::try_from("слово").is_ok());
    }

    #[test]
    fn try_from_spanish() {
        // "árbol" = á, r, b, o, l (5 NFC scalars)
        assert!(Word::<5>::try_from("árbol").is_ok());
    }

    #[test]
    fn try_from_nfd_wrong_char_count() {
        // try_from counts raw Unicode scalars, so NFD input (e.g. two codepoints for é) fails;
        // NFC normalization is the caller's responsibility before constructing a Word.
        let nfd = "a\u{0301}rbol";
        assert_eq!(nfd.chars().count(), 6);
        assert!(Word::<5>::try_from(nfd).is_err());
    }

    #[test]
    fn words_sorted_descending() {
        let ws = ws(&[("crane", 100), ("stare", 50), ("light", 200)]);
        let words: Vec<_> = ws.words().collect();
        assert_eq!(words[0], Word::try_from("light").unwrap()); // 200
        assert_eq!(words[1], Word::try_from("crane").unwrap()); // 100
        assert_eq!(words[2], Word::try_from("stare").unwrap()); // 50
    }

    #[test]
    fn probabilities_normalized() {
        let ws = ws(&[("crane", 100), ("stare", 200), ("light", 300)]);
        let probs = ws.probabilities();
        assert_eq!(probs.len(), 3);
        let sum: f64 = probs.iter().copied().sum();
        assert!((sum - 1.0).abs() < 1e-10, "probabilities sum to {sum}");
    }

    #[test]
    fn limit_truncates() {
        let mut ws = ws(&[
            ("crane", 100),
            ("stare", 50),
            ("light", 200),
            ("mount", 75),
            ("swipe", 25),
        ]);
        ws.limit(2);
        assert_eq!(ws.len(), 2);
        assert!(ws.contains(&Word::try_from("light").unwrap()));
        assert!(ws.contains(&Word::try_from("crane").unwrap()));
    }

    #[test]
    fn limit_larger_than_set() {
        let mut ws = ws(&[("crane", 100), ("stare", 50)]);
        ws.limit(100);
        assert_eq!(ws.len(), 2);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn try_from_succeeds_iff_correct_char_count(
            exact in "[a-z]{5}",
            too_short in "[a-z]{1,4}",
            too_long in "[a-z]{6,10}",
        ) {
            prop_assert!(Word::<5>::try_from(exact.as_str()).is_ok());
            prop_assert!(Word::<5>::try_from(too_short.as_str()).is_err());
            prop_assert!(Word::<5>::try_from(too_long.as_str()).is_err());
        }
    }
}
