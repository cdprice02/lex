use lex_data::Word;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Correctness {
    Absent,
    Misplaced,
    Correct,
}

impl Correctness {
    pub const COUNT: usize = std::mem::variant_count::<Self>();
}

impl std::fmt::Display for Correctness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            Correctness::Absent => '⬜',
            Correctness::Misplaced => '🟨',
            Correctness::Correct => '🟩',
        };
        write!(f, "{}", symbol)
    }
}

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
    pub const COUNT: u64 = (Correctness::COUNT as u64).pow(N as u32);

    pub fn ordinal(&self) -> u64 {
        self.iter()
            .fold(0, |acc, &c| acc * Correctness::COUNT as u64 + c as u64)
    }

    pub fn absent() -> Self {
        Self([Correctness::Absent; N])
    }

    /// The feedback for playing `guess` against `word`.
    pub fn correct(word: Word<N>, guess: Word<N>) -> Self {
        Self::from_ordinal(pattern_ordinal(&word, &guess))
    }

    /// Inverse of [`ordinal`](Self::ordinal). The ordinal must be below [`COUNT`](Self::COUNT).
    fn from_ordinal(mut ordinal: u64) -> Self {
        debug_assert!(ordinal < Self::COUNT);
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

    pub fn is_correct(&self) -> bool {
        self.0.iter().all(|&c| c == Correctness::Correct)
    }
}

/// The [`ordinal`](WordCorrectness::ordinal) of the feedback for guessing `guess` against `word`,
/// over any letter type, so the solver can compare alphabet indices instead of chars.
///
/// A guess letter is `Correct` when it equals the word's letter at its position; otherwise it is
/// `Misplaced` if an unmatched word letter equal to it is still unclaimed (each word letter is
/// claimed at most once, left to right), else `Absent`. O(N^2) with two position bitmasks and no
/// shared state: for N up to 10 that beats a count table, whose scattered cache lines cost more
/// than the extra compares.
pub(crate) fn pattern_ordinal<T: Copy + Eq, const N: usize>(word: &[T; N], guess: &[T; N]) -> u64 {
    let mut green = 0u32;
    for (i, (w, g)) in word.iter().zip(guess).enumerate() {
        if w == g {
            green |= 1 << i;
        }
    }
    // word positions no longer available to a misplaced match: the green ones, then each claimed
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
            match unclaimed {
                Some(j) => {
                    claimed |= 1 << j;
                    Correctness::Misplaced as u64
                }
                None => Correctness::Absent as u64,
            }
        };
        ordinal = ordinal * Correctness::COUNT as u64 + digit;
    }
    ordinal
}

impl<const N: usize> std::fmt::Display for WordCorrectness<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for state in &self.0 {
            write!(f, "{}", state)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod benches {
    extern crate test;
    use std::hint::black_box;
    use test::Bencher;

    use super::*;

    #[bench]
    fn correct_no_overlap(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("fling").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    #[bench]
    fn correct_partial_match(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("track").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    #[bench]
    fn ordinal_no_overlap(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("fling").unwrap();
        b.iter(|| black_box(pattern_ordinal(&word, &guess)));
    }

    #[bench]
    fn ordinal_partial_match(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("track").unwrap();
        b.iter(|| black_box(pattern_ordinal(&word, &guess)));
    }

    #[bench]
    fn ordinal_perfect_match(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("crate").unwrap();
        b.iter(|| black_box(pattern_ordinal(&word, &guess)));
    }

    #[bench]
    fn ordinal_partial_match_n10(b: &mut Bencher) {
        let word = Word::<10>::try_from("cartridges").unwrap();
        let guess = Word::<10>::try_from("registrant").unwrap();
        b.iter(|| black_box(pattern_ordinal(&word, &guess)));
    }

    #[bench]
    fn correct_perfect_match(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("crate").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
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
            prop_assert_eq!(pattern_ordinal(&word, &guess), reference(&word, &guess));
            let (w5, g5): ([u8; 5], [u8; 5]) =
                (word[..5].try_into().unwrap(), guess[..5].try_into().unwrap());
            prop_assert_eq!(pattern_ordinal(&w5, &g5), reference(&w5, &g5));
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
