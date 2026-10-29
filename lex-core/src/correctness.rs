//! Per-letter and per-guess feedback, and the base-3 ordinals that index feedback patterns.

use lex_data::Word;

/// Feedback for one letter of a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumCount)]
pub enum Correctness {
    /// The letter is absent from the answer, or every occurrence is already accounted for.
    Absent,
    /// The letter is in the answer at another position.
    Misplaced,
    /// The letter is in the answer at this position.
    Correct,
}

impl Correctness {
    /// Number of feedback kinds per letter.
    pub const COUNT: usize = <Self as strum::EnumCount>::COUNT;
}

impl std::fmt::Display for Correctness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            Self::Absent => '⬜',
            Self::Misplaced => '🟨',
            Self::Correct => '🟩',
        };
        write!(f, "{symbol}")
    }
}

/// Feedback for a whole guess, one [`Correctness`] per letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordCorrectness<const N: usize>([Correctness; N]);

// Ordinals are `u64`: 3^MAX_WORD_LENGTH must fit.
const _: () = assert!(
    3u128.pow(lex_data::MAX_WORD_LENGTH as u32) <= u64::MAX as u128,
    "pattern ordinals must fit u64"
);

impl<const N: usize> std::ops::Deref for WordCorrectness<N> {
    type Target = [Correctness; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> std::ops::DerefMut for WordCorrectness<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const N: usize> WordCorrectness<N> {
    /// Number of distinct feedback patterns, 3^N.
    pub const COUNT: u64 = (Correctness::COUNT as u64).pow(N as u32);

    /// The pattern's base-3 ordinal (absent 0, misplaced 1, correct 2; position 0 most significant), below [`COUNT`](Self::COUNT).
    #[must_use]
    pub fn ordinal(&self) -> u64 {
        self.iter()
            .fold(0, |acc, &c| acc * Correctness::COUNT as u64 + c as u64)
    }

    /// The all-absent pattern, ordinal 0.
    #[must_use]
    pub const fn absent() -> Self {
        Self([Correctness::Absent; N])
    }

    /// The feedback for playing `guess` against `word`.
    #[must_use]
    pub fn correct(word: Word<N>, guess: Word<N>) -> Self {
        // Index each letter by where it first appears in `word`, else in `guess`: at most 2N
        // distinct values, so no Alphabet is needed to compare a pair.
        // `at` is where `c` sits in its own word, so it stands in for the position lookup that
        // cannot miss
        let first = |c: char, at: usize| {
            word.iter()
                .position(|&w| w == c)
                .unwrap_or_else(|| N + guess.iter().position(|&g| g == c).unwrap_or(at))
                as u8
        };
        let word_code: Code<N> = std::array::from_fn(|i| first(word[i], i));
        let guess_code: Code<N> = std::array::from_fn(|i| first(guess[i], i));
        Self::from_ordinal(pattern_ordinal(&word_code, &guess_code, &mut [0; 256]))
    }

    /// Inverse of [`ordinal`](Self::ordinal). The ordinal must be below [`COUNT`](Self::COUNT).
    pub(crate) fn from_ordinal(mut ordinal: u64) -> Self {
        debug_assert!(ordinal < Self::COUNT, "ordinal {ordinal} is not below 3^N");
        let mut result = Self::absent();
        for slot in result.0.iter_mut().rev() {
            *slot = match ordinal % Correctness::COUNT as u64 {
                0 => Correctness::Absent,
                1 => Correctness::Misplaced,
                _ => Correctness::Correct,
            };
            ordinal /= Correctness::COUNT as u64;
        }
        result
    }

    /// Whether every letter is correct, so the guess is the answer.
    #[must_use]
    pub fn is_correct(&self) -> bool {
        self.0.iter().all(|&c| c == Correctness::Correct)
    }
}

/// A word's letters as indices into the Corpus Alphabet, one byte each.
pub type Code<const N: usize> = [u8; N];

/// The code of a letter outside the Alphabet. The Alphabet holds at most
/// `lex_data::MAX_ALPHABET_SIZE` (255) letters, so this value is never an Alphabet index.
pub const UNKNOWN_LETTER: u8 = u8::MAX;

/// Per-letter tally of unmatched word letters. All zero between calls to [`pattern_ordinal`],
/// so a caller makes one on its stack and reuses it across a whole scan.
pub type LetterCounts = [u8; 256];

/// The [`ordinal`](WordCorrectness::ordinal) of the feedback for guessing `guess` against `word`.
///
/// A guess letter is `Correct` when it equals the word's letter at its position; otherwise it is
/// `Misplaced` if an unmatched word letter equal to it is still unspent (each is spent at most
/// once, left to right), else `Absent`. The unmatched word letters are tallied per letter, so the
/// cost is linear in N at every length. `counts` must be all zero; the call zeroes the entries it
/// touched before returning.
pub fn pattern_ordinal<const N: usize>(
    word: &Code<N>,
    guess: &Code<N>,
    counts: &mut LetterCounts,
) -> u64 {
    for (&w, &g) in word.iter().zip(guess) {
        if w != g {
            counts[w as usize] += 1;
        }
    }
    let mut ordinal = 0;
    for (&w, &g) in word.iter().zip(guess) {
        let digit = if w == g {
            Correctness::Correct as u64
        } else if counts[g as usize] > 0 {
            counts[g as usize] -= 1;
            Correctness::Misplaced as u64
        } else {
            Correctness::Absent as u64
        };
        ordinal = ordinal * Correctness::COUNT as u64 + digit;
    }
    for &w in word {
        counts[w as usize] = 0;
    }
    ordinal
}

impl<const N: usize> std::fmt::Display for WordCorrectness<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for state in &self.0 {
            write!(f, "{state}")?;
        }
        Ok(())
    }
}

/// The O(N^2) compare that the letter count replaced, kept as an oracle for tests and as the
/// "old" side of the scaling benches.
#[cfg(any(test, feature = "bench"))]
fn compare_ordinal<const N: usize>(word: &Code<N>, guess: &Code<N>) -> u64 {
    let mut green = 0u32;
    for (i, (w, g)) in word.iter().zip(guess).enumerate() {
        if w == g {
            green |= 1 << i;
        }
    }
    let mut claimed = green;
    let mut ordinal = 0;
    for (i, &letter) in guess.iter().enumerate() {
        let digit = if green >> i & 1 == 1 {
            Correctness::Correct as u64
        } else {
            let unclaimed = word
                .iter()
                .enumerate()
                .position(|(j, &w)| claimed >> j & 1 == 0 && w == letter);
            unclaimed.map_or(Correctness::Absent as u64, |j| {
                claimed |= 1 << j;
                Correctness::Misplaced as u64
            })
        };
        ordinal = ordinal * Correctness::COUNT as u64 + digit;
    }
    ordinal
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
    use std::hint::black_box;

    use super::{Code, LetterCounts, Word, WordCorrectness, compare_ordinal, pattern_ordinal};

    #[divan::bench]
    fn correct_no_overlap(b: Bencher) {
        let word = Word::from(['c', 'r', 'a', 't', 'e']);
        let guess = Word::from(['f', 'l', 'i', 'n', 'g']);
        b.bench(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    #[divan::bench]
    fn correct_partial_match(b: Bencher) {
        let word = Word::from(['c', 'r', 'a', 't', 'e']);
        let guess = Word::from(['t', 'r', 'a', 'c', 'k']);
        b.bench(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    /// Reproducible word pairs over a..z, so letters repeat and overlap as in a real corpus.
    fn pairs<const N: usize>() -> Vec<(Code<N>, Code<N>)> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut word = || -> Code<N> {
            std::array::from_fn(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 33) % 26) as u8
            })
        };
        (0..256).map(|_| (word(), word())).collect()
    }

    #[divan::bench(consts = [5, 10, 13, 25])]
    fn ordinal_count<const N: usize>(b: Bencher) {
        let pairs = pairs::<N>();
        let mut counts: LetterCounts = [0; 256];
        b.bench_local(|| {
            for (w, g) in &pairs {
                black_box(pattern_ordinal(black_box(w), black_box(g), &mut counts));
            }
        });
    }

    #[divan::bench(consts = [5, 10, 13, 25])]
    fn ordinal_compare<const N: usize>(b: Bencher) {
        let pairs = pairs::<N>();
        b.bench(|| {
            for (w, g) in &pairs {
                black_box(compare_ordinal(black_box(w), black_box(g)));
            }
        });
    }

    #[divan::bench]
    fn correct_perfect_match(b: Bencher) {
        let word = Word::from(['c', 'r', 'a', 't', 'e']);
        let guess = Word::from(['c', 'r', 'a', 't', 'e']);
        b.bench(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_positions_correct() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("crate").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([Correctness::Correct; 5])
        );
    }

    #[test]
    fn all_positions_absent() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("fling").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([Correctness::Absent; 5])
        );
    }

    #[test]
    fn misplaced_and_correct_mix() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("trace").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Misplaced,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn correct_prefix_all_misplaced() {
        let word = Word::try_from("train").unwrap();
        let guess = Word::try_from("trina").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Misplaced,
                Correctness::Misplaced
            ])
        );
    }

    #[test]
    fn duplicate_pool_exhausted() {
        // word has one unmatched 'l'; guess has two — second 'l' is Absent once the pool is used up
        let word = Word::try_from("apple").unwrap();
        let guess = Word::try_from("allee").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn mostly_correct_trailing_absent() {
        let word = Word::try_from("stats").unwrap();
        let guess = Word::try_from("state").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Absent
            ])
        );
    }

    #[test]
    fn unicode_bmp_chars() {
        let word = Word::try_from("cœurs").unwrap();
        let guess = Word::try_from("sœurs").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Absent,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
            ])
        );
    }

    #[test]
    fn count_is_base_to_the_n() {
        assert_eq!(WordCorrectness::<5>::COUNT, 243); // 3^5
    }

    #[test]
    fn duplicate_in_word_extra_absent() {
        // both 'a's in the word are exactly matched, leaving no pool for the third 'a' in guess
        let word = Word::try_from("aabcd").unwrap();
        let guess = Word::try_from("aaaef").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
            ])
        );
    }

    #[test]
    fn duplicate_in_guess_capped() {
        // word has one 'e' (exactly matched); the other two 'e's in guess get no credit
        let word = Word::try_from("crane").unwrap();
        let guess = Word::try_from("eerie").unwrap();
        assert_eq!(
            WordCorrectness::<5>::correct(word, guess),
            WordCorrectness([
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Misplaced,
                Correctness::Absent,
                Correctness::Correct,
            ])
        );
    }

    #[test]
    fn ordinal_boundary_values() {
        // all-absent = 0; all-correct = COUNT-1 (both digits at extreme of base-B)
        assert_eq!(WordCorrectness::<5>::absent().ordinal(), 0);
        assert_eq!(
            WordCorrectness([Correctness::Correct; 5]).ordinal(),
            WordCorrectness::<5>::COUNT - 1
        );
        // [Correct, Absent×4] = 2 * 3^4 = 162 (big-endian, position 0 is MSB)
        assert_eq!(
            WordCorrectness([
                Correctness::Correct,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
            ])
            .ordinal(),
            162
        );
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        /// The pattern ordinal agrees with a literal reading of the Wordle rules (count the
        /// unmatched word letters, spend them left to right) on tiny alphabets, where repeated
        /// letters are the norm.
        #[test]
        fn pattern_ordinal_matches_the_count_rule(
            word in proptest::array::uniform10(0u8..4),
            guess in proptest::array::uniform10(0u8..4),
        ) {
            fn reference<const N: usize>(word: &[u8; N], guess: &[u8; N]) -> u64 {
                let mut counts = [0usize; 4];
                for i in 0..N {
                    if word[i] != guess[i] {
                        counts[word[i] as usize] += 1;
                    }
                }
                let mut ordinal = 0;
                for i in 0..N {
                    let digit = if word[i] == guess[i] {
                        2
                    } else if counts[guess[i] as usize] > 0 {
                        counts[guess[i] as usize] -= 1;
                        1
                    } else {
                        0
                    };
                    ordinal = ordinal * 3 + digit;
                }
                ordinal
            }
            let mut counts: LetterCounts = [0; 256];
            prop_assert_eq!(
                pattern_ordinal(&word, &guess, &mut counts),
                reference(&word, &guess)
            );
            prop_assert_eq!(compare_ordinal(&word, &guess), reference(&word, &guess));
            let (w5, g5): ([u8; 5], [u8; 5]) =
                (word[..5].try_into().unwrap(), guess[..5].try_into().unwrap());
            prop_assert_eq!(pattern_ordinal(&w5, &g5, &mut counts), reference(&w5, &g5));
            prop_assert!(counts.iter().all(|&c| c == 0), "the table is left zeroed");
        }

        /// Wide alphabets and the longest lengths, where one table slot per letter matters.
        #[test]
        fn count_matches_compare_n25_over_wide_alphabet(
            word in proptest::array::uniform25(0u8..=255),
            guess in proptest::array::uniform25(0u8..=255),
        ) {
            let mut counts: LetterCounts = [0; 256];
            prop_assert_eq!(
                pattern_ordinal(&word, &guess, &mut counts),
                compare_ordinal(&word, &guess)
            );
            prop_assert!(counts.iter().all(|&c| c == 0));
        }

        #[test]
        fn ordinal_round_trips_through_correctness(ordinal in 0u64..243) {
            prop_assert_eq!(WordCorrectness::<5>::from_ordinal(ordinal).ordinal(), ordinal);
        }

        #[test]
        fn self_guess_all_correct(word_str in "[a-z]{5}") {
            let word = Word::<5>::try_from(word_str.as_str()).unwrap();
            prop_assert_eq!(
                WordCorrectness::<5>::correct(word, word),
                WordCorrectness([Correctness::Correct; 5])
            );
        }

    }
}
