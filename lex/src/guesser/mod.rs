use std::collections::HashMap;
use std::hint::cold_path;

use lex_data::Word;
use lex_data::WordSet;

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

// TODO: when a second strategy is needed, extract a GuesserStrategy trait from this API —
// the current method signatures are trait-compatible (push_guess + suggest).
pub struct Guesser<const N: usize> {
    word_set: WordSet<N>,
    history: Vec<Guess<N>>,
}

impl<const N: usize> Guesser<N> {
    pub fn new(word_set: WordSet<N>) -> Self {
        Self {
            word_set,
            history: Vec::new(),
        }
    }

    pub fn history(&self) -> &[Guess<N>] {
        &self.history
    }

    pub fn push_guess(&mut self, guess: Guess<N>) {
        self.history.push(guess);
    }

    pub fn suggest(&mut self) -> Option<Word<N>> {
        if self.word_set.is_empty() {
            cold_path();
            return None;
        }

        if let Some(last) = self.history.last() {
            let last_word = last.word();
            let last_correctness = last.correctness();
            self.word_set
                .retain(|w| WordCorrectness::correct(*w, last_word) == last_correctness);
        }

        if self.word_set.is_empty() {
            cold_path();
            return None;
        }

        let probabilities = self.word_set.probabilities();
        let candidates = self.word_set.words();

        let mut best_i = 0;
        let mut best_score = guess_entropy(&candidates[0], &probabilities);
        for i in 1..candidates.len() {
            let score = guess_entropy(&candidates[i], &probabilities);
            log::trace!("{}: {}", candidates[i], score);
            if score > best_score {
                log::debug!(
                    "{} > {}; {} > {}",
                    candidates[i],
                    candidates[best_i],
                    score,
                    best_score
                );
                best_score = score;
                best_i = i;
            }
        }
        Some(candidates[best_i])
    }
}

#[optimize(speed)]
fn guess_entropy<const N: usize>(guess: &Word<N>, probabilities: &HashMap<Word<N>, f64>) -> f64 {
    let mut pattern_probs: HashMap<WordCorrectness<N>, f64> = WordCorrectness::<N>::all_possible()
        .map(|p| (p, 0.0))
        .collect();
    for (word, &prob) in probabilities {
        *pattern_probs
            .get_mut(&WordCorrectness::correct(*word, *guess))
            .expect("pattern not in pre-allocated map") += prob;
    }
    -pattern_probs
        .values()
        .filter(|&&p| p > 0.0)
        .map(|&p| p * p.log2())
        .sum::<f64>()
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
        let ws = WordSet::new(frequencies);
        Guesser::<5>::new(ws)
    }

    #[bench]
    fn guess_entropy_10_words(b: &mut Bencher) {
        let pairs = make_wordset(10);
        let guess = pairs[0].0;
        let frequencies: HashMap<Word<5>, u64> = pairs.iter().cloned().collect();
        let probabilities = WordSet::new(frequencies).probabilities();
        b.iter(|| black_box(guess_entropy(&guess, &probabilities)));
    }

    #[bench]
    fn suggest_50_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(50);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            g.push_guess(initial_guess.clone());
            black_box(g.suggest())
        });
    }

    #[bench]
    fn suggest_200_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(200);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            g.push_guess(initial_guess.clone());
            black_box(g.suggest())
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
        let ws = WordSet::new(frequencies);
        Guesser::<5>::new(ws)
    }

    fn word(s: &str) -> Word<5> {
        Word::<5>::try_from(s).unwrap()
    }

    fn make_guess(guess_str: &str, target_str: &str) -> Guess<5> {
        let guess_word = word(guess_str);
        let target_word = word(target_str);
        Guess::new(
            guess_word,
            WordCorrectness::correct(target_word, guess_word),
        )
    }

    #[test]
    fn suggest_no_prior_guesses_returns_word() {
        let all_words = ["crane", "stare", "light", "mount", "swipe"];
        let mut g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        let next = g.suggest().unwrap();
        assert!(
            all_words.iter().any(|&w| word(w) == next),
            "suggest returned {next}, which is not in the dictionary"
        );
    }

    #[test]
    fn suggest_returns_dictionary_member() {
        let all_words = ["crane", "stare", "light", "mount", "swipe"];
        let mut g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        g.push_guess(make_guess("crane", "stare"));
        let next = g.suggest().unwrap();
        assert!(
            all_words.iter().any(|&w| word(w) == next),
            "suggest returned {next}, which is not in the dictionary"
        );
    }

    //? this test might fail if we allow "impossible" words in the future for valuable information gain
    #[test]
    fn suggest_filters_impossible_words() {
        // words inconsistent with the last guess's correctness pattern are removed from the dictionary
        let mut g = make_guesser(&[
            ("crane", 100),
            ("crave", 100),
            ("craze", 100),
            ("graze", 100),
        ]);
        g.push_guess(make_guess("crane", "crave"));
        let next = g.suggest().unwrap();
        let valid = [word("crave"), word("craze")];
        assert!(
            valid.contains(&next),
            "expected crave or craze after filtering, got {next}"
        );
    }

    #[test]
    fn suggest_single_word() {
        let mut g = make_guesser(&[("crane", 100), ("crave", 200), ("graze", 100)]);
        g.push_guess(make_guess("crane", "crave"));
        let next = g.suggest().unwrap();
        assert_eq!(next, word("crave"));
    }

    #[test]
    fn suggest_exhausted_dictionary_returns_none() {
        let mut g = make_guesser(&[("crane", 100), ("stare", 100)]);
        // push a guess where the target matches neither word's pattern
        let impossible = Guess::new(word("crane"), WordCorrectness::absent());
        g.push_guess(impossible);
        assert_eq!(g.suggest(), None);
    }
}
