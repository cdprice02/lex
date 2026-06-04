use anyhow::Context;
use std::collections::HashMap;
use std::path::Path;

use lex_data::Language;
use lex_data::Word;

pub mod correctness;

pub use correctness::WordCorrectness;

#[derive(Debug, Clone)]
pub struct Guess<const N: usize> {
    word: Word<N>,
    correctness: WordCorrectness<N>,
}

impl<const N: usize> Guess<N> {
    pub fn new(word: Word<N>, correctness: WordCorrectness<N>) -> Self {
        Self { word, correctness }
    }

    pub fn word(&self) -> Word<N> {
        self.word
    }

    pub fn correctness(&self) -> WordCorrectness<N> {
        self.correctness
    }
}

// TODO: guess strategy or heuristic that takes into account the frequency of words in the dictionary; this would allow us to prioritize more common words and potentially solve the puzzle faster
// TODO: some sort of Trait for Guesser implementations or GuessStrategy
pub struct Guesser<const N: usize> {
    dictionary: Vec<Word<N>>,
    word_probabilities: HashMap<Word<N>, f64>,
    // TODO: Guesser holds guess history
}

impl<const N: usize> Guesser<N> {
    #[cfg(test)]
    fn from_word_set(word_set: lex_data::WordSet<N>) -> Self {
        Self {
            dictionary: word_set.words(),
            word_probabilities: word_set.probabilities(),
        }
    }

    pub fn try_new(
        data_dir: &Path,
        lang: Language,
        dictionary_length: Option<usize>,
    ) -> anyhow::Result<Self> {
        let word_set = lex_data::blocking::get::<N>(data_dir, lang, dictionary_length)
            .context("loading wordset")?;

        Ok(Self {
            dictionary: word_set.words(),
            word_probabilities: word_set.probabilities(),
        })
    }

    pub fn next_guess(&mut self, history: Vec<Guess<N>>) -> Word<N> {
        let last_guess = history
            .last()
            .expect("history should have at least one guess"); // TODO: allow guessing the first word

        self.dictionary.retain(|w| {
            let correctness = WordCorrectness::correct(*w, last_guess.word());
            correctness == last_guess.correctness()
        });

        // softmax the probabilities of the remaining words for the next guess
        let probabilities: HashMap<Word<N>, f64> = self
            .word_probabilities
            .iter()
            .filter(|(word, _)| self.dictionary.contains(word))
            .map(|(word, &prob)| (*word, prob.exp()))
            .collect();
        let sum: f64 = probabilities.values().sum();
        let probabilities: HashMap<Word<N>, f64> = probabilities
            .iter()
            .map(|(word, &prob)| (*word, prob / sum))
            .collect();

        let mut best_i = 0;
        let get_score =
            |word: &Word<N>| expected_information(word, Some(last_guess), probabilities.clone());
        let mut best_score = get_score(&self.dictionary[0]);
        for i in 1..self.dictionary.len() {
            let score = get_score(&self.dictionary[i]);
            log::trace!("{}: {}", self.dictionary[i], score);
            if score > best_score {
                log::debug!(
                    "{} > {}; {} > {}",
                    self.dictionary[i],
                    self.dictionary[best_i],
                    score,
                    best_score
                );
                best_score = score;
                best_i = i;
            }
        }
        self.dictionary[best_i]
    }
}

#[optimize(speed)]
fn expected_information<const N: usize>(
    guess: &Word<N>,
    previous_guess: Option<&Guess<N>>,
    probabilities: HashMap<Word<N>, f64>,
) -> f64 {
    let mut entropy = 0.0;
    let patterns: Vec<_> = match previous_guess {
        None => WordCorrectness::<N>::all_possible().collect(),
        Some(prev) => WordCorrectness::<N>::all_possible_from(prev.correctness()).collect(),
    };
    for pattern in patterns {
        let mut pattern_probability = 0.0;
        for (word, &prob) in &probabilities {
            if WordCorrectness::correct(*word, *guess) == pattern {
                pattern_probability += prob;
            }
        }
        if pattern_probability > 0.0 {
            entropy += pattern_probability * pattern_probability.log2();
        }
    }

    -entropy
}

#[cfg(test)]
mod benches {
    extern crate test;
    use std::collections::HashMap;
    use std::hint::black_box;
    use test::Bencher;

    use lex_data::{Word, WordSet};

    use super::*;
    use crate::guesser::correctness::WordCorrectness;

    fn make_wordset(n: usize) -> Vec<(Word<5>, u64)> {
        (0..n)
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
            .collect()
    }

    fn build_guesser(pairs: &[(Word<5>, u64)]) -> Guesser<5> {
        let frequencies: HashMap<Word<5>, u64> = pairs.iter().cloned().collect();
        Guesser::<5>::from_word_set(WordSet { frequencies })
    }

    #[bench]
    fn expected_information_10_words(b: &mut Bencher) {
        let pairs = make_wordset(10);
        let guess = pairs[0].0;
        let first_correctness = WordCorrectness::absent();
        let last_guess = Guess::new(guess, first_correctness);
        let probabilities: HashMap<Word<5>, f64> = pairs
            .iter()
            .enumerate()
            .map(|(i, &(w, _))| (w, 1.0 / (10.0 + i as f64)))
            .collect();
        b.iter(|| {
            black_box(expected_information(
                &guess,
                Some(&last_guess),
                probabilities.clone(),
            ))
        });
    }

    #[bench]
    fn next_guess_50_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(50);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let history = vec![Guess::new(
            first_guess,
            WordCorrectness::correct(target, first_guess),
        )];
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            black_box(g.next_guess(history.clone()))
        });
    }

    #[bench]
    fn next_guess_200_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(200);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let history = vec![Guess::new(
            first_guess,
            WordCorrectness::correct(target, first_guess),
        )];
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            black_box(g.next_guess(history.clone()))
        });
    }
}

#[cfg(test)]
mod tests {
    use lex_data::{Word, WordSet};

    use super::*;
    use crate::guesser::correctness::WordCorrectness;

    fn make_guesser(pairs: &[(&str, u64)]) -> Guesser<5> {
        let frequencies = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        Guesser::<5>::from_word_set(WordSet { frequencies })
    }

    fn word(s: &str) -> Word<5> {
        Word::<5>::try_from(s).unwrap()
    }

    fn history_of(guess_str: &str, target_str: &str) -> Vec<Guess<5>> {
        let guess_word = word(guess_str);
        let target_word = word(target_str);
        let correctness = WordCorrectness::correct(target_word, guess_word);
        vec![Guess::new(guess_word, correctness)]
    }

    #[test]
    fn next_guess_returns_dictionary_member() {
        let all_words = ["crane", "stare", "light", "mount", "swipe"];
        let mut g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        let next = g.next_guess(history_of("crane", "stare"));
        assert!(
            all_words.iter().any(|&w| word(w) == next),
            "next_guess returned {next}, which is not in the dictionary"
        );
    }

    #[test]
    fn next_guess_filters_impossible_words() {
        // words inconsistent with the last guess's correctness pattern are removed from the dictionary
        let mut g = make_guesser(&[
            ("crane", 100),
            ("crave", 100),
            ("craze", 100),
            ("graze", 100),
        ]);
        let next = g.next_guess(history_of("crane", "crave"));
        let valid = [word("crave"), word("craze")];
        assert!(
            valid.contains(&next),
            "expected crave or craze after filtering, got {next}"
        );
    }

    #[test]
    fn next_guess_single_word() {
        let mut g = make_guesser(&[("crane", 100), ("crave", 200), ("graze", 100)]);
        let next = g.next_guess(history_of("crane", "crave"));
        assert_eq!(next, word("crave"));
    }
}
