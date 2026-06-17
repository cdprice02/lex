use std::cell::RefCell;

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
    pub const COUNT: usize = Correctness::COUNT.pow(N as u32);

    pub fn ordinal(&self) -> usize {
        self.iter()
            .fold(0, |acc, &c| acc * Correctness::COUNT + c as usize)
    }

    pub fn absent() -> Self {
        Self([Correctness::Absent; N])
    }

    #[optimize(speed)]
    pub fn correct(word: Word<N>, guess: Word<N>) -> Self {
        thread_local! {
            /// Thread-local buffer sized for the full BMP (U+0000–U+FFFF), initialized once per thread
            static CHAR_COUNTS: RefCell<Box<[u8; 0x10000]>> = RefCell::new(Box::new([0; 0x10000]));
        }

        CHAR_COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            let mut result = Self::absent();

            // first pass for correct: if the guess char matches the word char at the same position, it's correct; otherwise, add the word char to the pool of unmatched chars
            for i in 0..N {
                let w_ch = word[i];
                if w_ch == guess[i] {
                    result[i] = Correctness::Correct;
                } else {
                    debug_assert!((w_ch as usize) < counts.len(), "char above U+FFFF");
                    counts[w_ch as usize] += 1;
                }
            }

            // second pass for misplaced: if the guess char exists in the pool of unmatched word chars, it's misplaced
            for i in 0..N {
                if result[i] == Correctness::Correct {
                    continue;
                }
                let idx = guess[i] as usize;
                if counts[idx] > 0 {
                    result[i] = Correctness::Misplaced;
                    counts[idx] -= 1;
                }
            }

            // reset counts for next call
            for i in 0..N {
                counts[word[i] as usize] = 0;
            }

            result
        })
    }

    pub fn is_correct(&self) -> bool {
        self.0.iter().all(|&c| c == Correctness::Correct)
    }
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
