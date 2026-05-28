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
    pub fn new(data_dir: &Path, lang: Language) -> Self {
        let word_set =
            lex_data::blocking::get::<N>(data_dir, lang).expect("failed to load word data");

        Self {
            dictionary: word_set.words(),
            word_probabilities: word_set.probabilities(),
        }
    }

    pub fn next_guess(&mut self, history: Vec<Guess<N>>) -> Word<N> {
        let last_guess = history
            .last()
            .expect("history should have at least one guess"); // TODO: allow guessing the first word

        self.dictionary.retain(|w| {
            let correctness = WordCorrectness::correct(*w, last_guess.word());
            correctness == last_guess.correctness()
        });

        let mut best_i = 0;
        let get_score = |word: &Word<N>| {
            expected_information(
                word,
                self.word_probabilities.clone(),
                self.dictionary.clone(),
                Some(last_guess),
            )
        };
        let mut best_score = get_score(&self.dictionary[0]);
        for i in 1..self.dictionary.len() {
            let score = get_score(&self.dictionary[i]);
            if score > best_score {
                eprintln!(
                    "{} > {}; {} > {}",
                    self.dictionary[i], self.dictionary[best_i], score, best_score
                );
                best_score = score;
                best_i = i;
            }
        }
        self.dictionary[best_i]
    }
}

fn expected_information<const N: usize>(
    guess: &Word<N>,
    probabilities: HashMap<Word<N>, f64>,
    dictionary: Vec<Word<N>>,
    previous_guess: Option<&Guess<N>>,
) -> f64 {
    let probabilities: HashMap<Word<N>, f64> = probabilities
        .iter()
        .filter(|(word, _)| dictionary.contains(word))
        .map(|(word, &prob)| (*word, prob.exp()))
        .collect();
    let sum: f64 = probabilities.values().sum();
    let probabilities: HashMap<Word<N>, f64> = probabilities
        .iter()
        .map(|(word, &prob)| (*word, prob / sum))
        .collect();

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
