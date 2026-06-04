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

#[cfg(test)]
mod benches {
    extern crate test;
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
            .collect();
        WordSet { frequencies }
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
    use super::*;

    fn ws(pairs: &[(&str, u64)]) -> WordSet<5> {
        let frequencies = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        WordSet { frequencies }
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
        let words = ws.words();
        assert_eq!(words[0], Word::try_from("light").unwrap()); // 200
        assert_eq!(words[1], Word::try_from("crane").unwrap()); // 100
        assert_eq!(words[2], Word::try_from("stare").unwrap()); // 50
    }

    #[test]
    fn probabilities_normalized() {
        let ws = ws(&[("crane", 100), ("stare", 200), ("light", 300)]);
        let probs = ws.probabilities();
        assert_eq!(probs.len(), 3);
        let sum: f64 = probs.values().sum();
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
        assert!(
            ws.frequencies
                .contains_key(&Word::try_from("light").unwrap())
        );
        assert!(
            ws.frequencies
                .contains_key(&Word::try_from("crane").unwrap())
        );
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
