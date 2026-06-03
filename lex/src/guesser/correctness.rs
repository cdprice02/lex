use std::cell::RefCell;

use lex_data::Word;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Correctness {
    Absent,
    Misplaced,
    Correct,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub fn all_possible() -> impl Iterator<Item = Self> {
        gen {
            const VARIANTS: [Correctness; 3] = [
                Correctness::Absent,
                Correctness::Misplaced,
                Correctness::Correct,
            ];
            let mut indices = [0usize; N];
            loop {
                yield Self(indices.map(|i| VARIANTS[i]));
                // increment mixed-radix counter (base 3, N digits, rightmost first)
                let mut pos = N;
                loop {
                    if pos == 0 {
                        return;
                    }
                    pos -= 1;
                    indices[pos] += 1;
                    if indices[pos] < 3 {
                        break;
                    }
                    indices[pos] = 0;
                }
            }
        }
    }

    // TODO: re-explore options for this generation
    pub fn all_possible_from(prev: Self) -> impl Iterator<Item = Self> {
        Self::all_possible().filter(move |pattern| {
            pattern
                .0
                .iter()
                .zip(prev.0.iter())
                .all(|(p, prev_p)| *prev_p != Correctness::Correct || *p == Correctness::Correct)
        })
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
    fn correctness_incorrect(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("fling").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    #[bench]
    fn correctness_sorta_correct(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("track").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }

    #[bench]
    fn correctness_correct(b: &mut Bencher) {
        let word = Word::<5>::try_from("crate").unwrap();
        let guess = Word::<5>::try_from("crate").unwrap();
        b.iter(|| black_box(WordCorrectness::<5>::correct(word, guess)));
    }
}

// TODO: more comprehensive tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correct() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("crate").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_correct_short() {
        let word = Word::try_from("ace").unwrap();
        let guess = Word::try_from("ace").unwrap();
        let correctness = WordCorrectness::<3>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_incorrect() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("fling").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent
            ])
        );
    }

    #[test]
    fn test_incorrect_short() {
        let word = Word::try_from("ace").unwrap();
        let guess = Word::try_from("bug").unwrap();
        let correctness = WordCorrectness::<3>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
            ])
        );
    }

    #[test]
    fn test_partial_1() {
        let word = Word::try_from("crate").unwrap();
        let guess = Word::try_from("trace").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
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
    fn test_partial_2() {
        let word = Word::try_from("train").unwrap();
        let guess = Word::try_from("trina").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
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
    fn test_mixed_1() {
        let word = Word::try_from("apple").unwrap();
        let guess = Word::try_from("allee").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
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
    fn test_mixed_2() {
        let word = Word::try_from("stats").unwrap();
        let guess = Word::try_from("state").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
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
    fn test_mixed_3() {
        let word = Word::try_from("zesty").unwrap();
        let guess = Word::try_from("trace").unwrap();
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Misplaced,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Misplaced
            ])
        );
    }
}
