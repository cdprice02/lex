use lex_data::Word;

use exhaust::Exhaust;
use itertools::Itertools;
use std::hash::Hash;
use std::iter::Iterator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Exhaust)]
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

impl<const N: usize> WordCorrectness<N> {
    pub fn all_possible() -> impl Iterator<Item = Self> {
        let mut all_pattern_options = Vec::new();
        for _ in 0..N {
            all_pattern_options.push(Correctness::exhaust());
        }

        all_pattern_options
            .into_iter()
            .multi_cartesian_product()
            .map(|pattern| Self(*pattern.as_array::<N>().expect("length checked above")))
    }

    pub fn all_possible_from(prev: Self) -> impl Iterator<Item = Self> {
        Self::all_possible().filter(move |pattern| {
            for (p, prev_p) in pattern.0.iter().zip(prev.0.iter()) {
                if *prev_p == Correctness::Correct && *p != Correctness::Correct {
                    return false;
                }
            }
            true
        })
    }

    pub fn absent() -> Self {
        Self([Correctness::Absent; N])
    }

    #[optimize(speed)]
    pub fn correct(word: Word<N>, guess: Word<N>) -> Self {
        let mut result = Self::absent();
        let mut used = [false; N];

        word.iter()
            .zip(guess)
            .enumerate()
            .for_each(|(i, (w_ch, g_ch))| {
                if *w_ch == g_ch {
                    result.0[i] = Correctness::Correct;
                    used[i] = true;
                } else if word.contains(&g_ch) {
                    result.0[i] = Correctness::Misplaced;
                } else {
                    result.0[i] = Correctness::Absent;
                }
            });

        for (i, g_ch) in guess.iter().enumerate() {
            if result.0[i] == Correctness::Misplaced {
                let mut found = false;
                for (j, w_ch) in word.iter().enumerate() {
                    if !used[j] && w_ch == g_ch {
                        used[j] = true;
                        found = true;
                        break;
                    }
                }
                if !found {
                    result.0[i] = Correctness::Absent;
                }
            }
        }

        result
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
