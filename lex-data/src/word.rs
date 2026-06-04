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

#[derive(Debug, Clone)]
pub struct WordSet<const N: usize> {
    // INVARIANT: words, frequencies, and scaled_probs are parallel and sorted descending
    // by frequency. Elements are only ever removed (via retain/limit) — never reordered
    // after construction. scaled_probs is recomputed eagerly after any mutation.
    words: Vec<Word<N>>,
    frequencies: Vec<u64>,
    scaled_probs: Vec<f64>,
}

impl<const N: usize> WordSet<N> {
    pub fn new(frequencies: HashMap<Word<N>, u64>) -> Self {
        let mut pairs: Vec<(Word<N>, u64)> = frequencies.into_iter().collect();
        pairs.sort_by_key(|&(_, f)| std::cmp::Reverse(f));
        let (words, frequencies): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        let scaled_probs = Self::compute_probs(&frequencies);
        Self {
            words,
            frequencies,
            scaled_probs,
        }
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

    /// Words sorted descending by frequency. Elements are only ever removed, never reordered.
    pub fn words(&self) -> &[Word<N>] {
        &self.words
    }

    /// Temperature-scaled probabilities (p_i ∝ f_i^(1/T)), parallel-indexed with `words()`.
    pub fn probabilities(&self) -> &[f64] {
        &self.scaled_probs
    }

    /// Iterator of `(word, probability)` pairs in frequency-descending order.
    pub fn word_probs(&self) -> impl Iterator<Item = (Word<N>, f64)> + '_ {
        self.words
            .iter()
            .copied()
            .zip(self.scaled_probs.iter().copied())
    }

    pub fn frequency(&self, word: &Word<N>) -> Option<u64> {
        self.words
            .iter()
            .position(|w| w == word)
            .map(|i| self.frequencies[i])
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn contains(&self, word: &Word<N>) -> bool {
        self.words.contains(word)
    }

    pub fn limit(&mut self, n: usize) {
        self.words.truncate(n);
        self.frequencies.truncate(n);
        self.scaled_probs = Self::compute_probs(&self.frequencies);
    }

    pub fn retain<F: Fn(&Word<N>) -> bool>(&mut self, predicate: F) {
        // INVARIANT preserved: zip-collect maintains relative order.
        (self.words, self.frequencies) = self
            .words
            .iter()
            .zip(self.frequencies.iter())
            .filter(|(w, _)| predicate(w))
            .map(|(&w, &f)| (w, f))
            .unzip();
        self.scaled_probs = Self::compute_probs(&self.frequencies);
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
        WordSet::new(frequencies)
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
        WordSet::new(frequencies)
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
