//! `Word` and `WordSet`, the in-memory form of a Corpus.

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use memmap2::{Mmap, MmapMut};

use crate::cast::{index, offset};
use crate::error::LexDataError;
use crate::le::array_at;

/// A word of exactly `N` letters.
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
        let mut letters = ['\0'; N];
        for (slot, ch) in letters.iter_mut().zip(s.chars()) {
            *slot = ch;
        }
        Ok(Self::from(letters))
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
            write!(f, "{ch}")?;
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
    /// Bytes of one stored letter.
    pub(crate) const CHAR_BYTES: usize = std::mem::size_of::<u32>();
    /// Bytes of the stored frequency.
    pub(crate) const FREQ_BYTES: usize = std::mem::size_of::<u64>();
    /// Bytes of one record.
    pub(crate) const SIZE: usize = Self::CHAR_BYTES * N + Self::FREQ_BYTES;

    /// The word with any invalid stored scalar replaced by U+FFFD, for records already validated
    /// at load.
    pub(crate) fn word_lossy(&self) -> Word<N> {
        Word::from(std::array::from_fn(|i| {
            char::from_u32(u32::from_le_bytes(array_at(self.0, i * Self::CHAR_BYTES)))
                .unwrap_or(char::REPLACEMENT_CHARACTER)
        }))
    }

    /// Returns `None` if any stored scalar is not a valid Unicode scalar value.
    pub(crate) fn word(&self) -> Option<Word<N>> {
        let mut chars = ['\0'; N];
        for (i, char_slot) in chars.iter_mut().enumerate() {
            let off = i * Self::CHAR_BYTES;
            let scalar = u32::from_le_bytes(array_at(self.0, off));
            *char_slot = char::from_u32(scalar)?;
        }
        Some(Word::from(chars))
    }

    /// The word's frequency.
    pub(crate) fn freq(&self) -> u64 {
        let off = Self::CHAR_BYTES * N;
        u64::from_le_bytes(array_at(self.0, off))
    }
}

impl<'a, const N: usize> From<&'a [u8]> for RawRecord<'a, N> {
    fn from(bytes: &'a [u8]) -> Self {
        debug_assert_eq!(bytes.len(), Self::SIZE, "a record is exactly SIZE bytes");
        RawRecord(bytes)
    }
}

/// Iterator over the active words in a [`WordSet`], reading lazily from the mmap backing store.
///
/// Implements [`Clone`] to support `.cycle()`.
pub struct WordIter<'a, const N: usize> {
    /// The records the iterator reads.
    mmap: &'a Mmap,
    /// Offsets of the records still to read.
    offsets: &'a [u32],
    /// Position of the next record in `offsets`.
    index: usize,
}

impl<const N: usize> Iterator for WordIter<'_, N> {
    type Item = Word<N>;

    fn next(&mut self) -> Option<Word<N>> {
        let &off = self.offsets.get(self.index)?;
        self.index += 1;
        let record =
            RawRecord::<N>::from(&self.mmap[index(off)..index(off) + RawRecord::<N>::SIZE]);
        Some(record.word_lossy())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.offsets.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<const N: usize> ExactSizeIterator for WordIter<'_, N> {}

impl<const N: usize> Clone for WordIter<'_, N> {
    fn clone(&self) -> Self {
        Self {
            mmap: self.mmap,
            offsets: self.offsets,
            index: self.index,
        }
    }
}

/// Words of one length in frequency-descending order, read from a memory map.
#[derive(Debug, Clone)]
pub struct WordSet<const N: usize> {
    /// The records the offsets point into.
    mmap: Arc<Mmap>,
    /// Absolute byte offsets of active records within the mmap (past any header), sorted descending
    /// by frequency. Elements are only ever removed (via limit), never reordered after construction.
    offsets: Vec<u32>,
}

impl<const N: usize> WordSet<N> {
    /// Constructs a `WordSet` from a memory-mapped ngrams cache whose records start at byte
    /// `records_start` and number `record_count`.
    ///
    /// Every record is checked once, here: each scalar must be a valid `char`, and
    /// frequencies must be positive and non-increasing (the solver and [`limit`](Self::limit)
    /// rely on the sort). After this, iteration can assume validity. All records are
    /// validated even when `limit` keeps only a prefix.
    pub(crate) fn from_cache(
        mmap: Mmap,
        path: &Path,
        records_start: usize,
        record_count: usize,
        limit: Option<usize>,
    ) -> Result<Self, LexDataError> {
        let bad = |index: usize, reason: &str| LexDataError::CorruptRecord {
            path: path.to_path_buf(),
            index,
            reason: reason.to_string(),
        };
        let size = RawRecord::<N>::SIZE;
        let mut prev_freq = u64::MAX;
        for index in 0..record_count {
            let off = records_start + index * size;
            let record = RawRecord::<N>::from(&mmap[off..off + size]);
            if record.word().is_none() {
                return Err(bad(index, "stored scalar is not a valid char"));
            }
            let freq = record.freq();
            if freq == 0 {
                return Err(bad(index, "frequency is zero"));
            }
            if freq > prev_freq {
                return Err(bad(index, "frequency exceeds the previous record's"));
            }
            prev_freq = freq;
        }
        let take = limit.map_or(record_count, |l| l.min(record_count));
        let offsets: Vec<u32> = (0..take)
            .map(|i| u32::try_from(records_start + i * size))
            .collect::<Result<_, _>>()
            .map_err(|_| bad(take, "record offset exceeds 4 GiB"))?;
        Ok(Self::from_offsets(mmap, offsets))
    }

    /// A set over `mmap` holding the records at `offsets`.
    fn from_offsets(mmap: Mmap, offsets: Vec<u32>) -> Self {
        Self {
            mmap: Arc::new(mmap),
            offsets,
        }
    }

    /// Constructs a `WordSet` from a word-to-frequency map.
    ///
    /// Sorts by frequency descending and writes records into an anonymous memory map.
    #[must_use]
    pub fn from_frequency_map(frequencies: HashMap<Word<N>, u64>) -> Self {
        Self::from_pairs(frequencies.into_iter().collect())
    }

    /// Constructs a `WordSet` from `(word, frequency)` pairs.
    ///
    /// The sort by frequency descending is stable, so equal frequencies keep the input
    /// order. Unlike [`from_frequency_map`](Self::from_frequency_map), whose tie order
    /// inherits `HashMap` iteration order, this is deterministic for fixtures and tests.
    ///
    /// # Panics
    ///
    /// If the operating system cannot map the records' memory.
    #[must_use]
    pub fn from_pairs(mut pairs: Vec<(Word<N>, u64)>) -> Self {
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
                    .copy_from_slice(&u32::from(ch).to_le_bytes());
            }
            let freq_off = off + RawRecord::<N>::CHAR_BYTES * N;
            buf[freq_off..freq_off + RawRecord::<N>::FREQ_BYTES]
                .copy_from_slice(&freq.to_le_bytes());
        }
        let mmap = buf.make_read_only().expect("failed to make mmap read-only");
        let offsets = (0..pairs.len()).map(|i| offset(i * record_size)).collect();
        Self::from_offsets(mmap, offsets)
    }

    /// Words sorted descending by frequency, read lazily from the mmap backing store.
    #[must_use]
    pub fn words(&self) -> WordIter<'_, N> {
        WordIter {
            mmap: &self.mmap,
            offsets: &self.offsets,
            index: 0,
        }
    }

    /// `(word, raw frequency)` pairs in frequency-descending order. The answer prior built on
    /// these frequencies is a solver modeling choice and lives in `lex-core`.
    pub fn word_frequencies(&self) -> impl Iterator<Item = (Word<N>, u64)> + '_ {
        self.offsets.iter().map(|&off| {
            let record =
                RawRecord::<N>::from(&self.mmap[index(off)..index(off) + RawRecord::<N>::SIZE]);
            (record.word_lossy(), record.freq())
        })
    }

    /// The frequency of `word`, or `None` when it is not in the set.
    #[must_use]
    pub fn frequency(&self, word: &Word<N>) -> Option<u64> {
        for &off in &self.offsets {
            let record =
                RawRecord::<N>::from(&self.mmap[index(off)..index(off) + RawRecord::<N>::SIZE]);
            if record.word().as_ref() == Some(word) {
                return Some(record.freq());
            }
        }
        None
    }

    /// The Alphabet: every distinct letter in the set's words.
    pub fn alphabet(&self) -> HashSet<char> {
        self.words()
            .flat_map(std::iter::IntoIterator::into_iter)
            .collect()
    }

    /// Fails if the Alphabet exceeds [`MAX_ALPHABET_SIZE`](crate::MAX_ALPHABET_SIZE), the most
    /// letters the solver can encode.
    pub(crate) fn ensure_alphabet_fits(&self) -> Result<(), LexDataError> {
        let size = self.alphabet().len();
        if size > crate::MAX_ALPHABET_SIZE {
            return Err(LexDataError::AlphabetTooLarge {
                size,
                max: crate::MAX_ALPHABET_SIZE,
            });
        }
        Ok(())
    }

    /// Number of words in the set.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Whether the set holds no words.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Whether `word` is in the set.
    #[must_use]
    pub fn contains(&self, word: &Word<N>) -> bool {
        self.frequency(word).is_some()
    }

    /// Keeps only the `n` most frequent words.
    pub fn limit(&mut self, n: usize) {
        self.offsets.truncate(n);
    }
}

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches {
    use divan::Bencher;
    use std::collections::HashMap;
    use std::hint::black_box;

    use super::{Word, WordSet};
    use crate::cast::widen;

    fn letter(i: usize) -> char {
        char::from(b"abcdefghijklmnopqrstuvwxyz"[i % 26])
    }

    fn make_wordset(n: usize) -> WordSet<5> {
        let frequencies = (0..n)
            .map(|i| {
                let word = Word::from([
                    letter(i),
                    letter(i / 26),
                    letter(i / 676),
                    letter(i / 17_576),
                    letter(i / 456_976),
                ]);
                (word, widen(n - i))
            })
            .collect::<HashMap<Word<5>, u64>>();
        WordSet::from_frequency_map(frequencies)
    }

    #[divan::bench]
    fn wordset_limit_1000_to_100(b: Bencher) {
        b.bench(|| {
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

    /// `letters` distinct letters, three to a word.
    fn with_letters(letters: u32) -> WordSet<3> {
        let chars: Vec<char> = (0x100..0x100 + letters)
            .filter_map(char::from_u32)
            .collect();
        let pairs = chars
            .chunks(3)
            .map(|c| {
                let word = std::array::from_fn(|i| c[i % c.len()]);
                (Word::from(word), 1)
            })
            .collect();
        WordSet::from_pairs(pairs)
    }

    #[test]
    fn alphabet_is_the_distinct_letters() {
        let a = ws(&[("crane", 3), ("nacre", 2)]).alphabet();
        let mut letters: Vec<char> = a.into_iter().collect();
        letters.sort_unstable();
        assert_eq!(letters, ['a', 'c', 'e', 'n', 'r']);
    }

    #[test]
    fn an_alphabet_of_255_letters_fits() {
        assert!(with_letters(255).ensure_alphabet_fits().is_ok());
    }

    #[test]
    fn an_alphabet_of_256_letters_is_a_typed_error() {
        let e = with_letters(256).ensure_alphabet_fits().unwrap_err();
        assert!(matches!(
            e,
            crate::error::LexDataError::AlphabetTooLarge {
                size: 256,
                max: 255
            }
        ));
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
    fn word_frequencies_follow_word_order() {
        let ws = ws(&[("crane", 100), ("stare", 200), ("light", 300)]);
        let pairs: Vec<_> = ws.word_frequencies().collect();
        assert_eq!(
            pairs,
            vec![
                (Word::try_from("light").unwrap(), 300),
                (Word::try_from("stare").unwrap(), 200),
                (Word::try_from("crane").unwrap(), 100),
            ]
        );
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
