use std::cell::RefCell;
use std::hint::cold_path;

use lex_data::Word;
use lex_data::WordSet;

use crate::correctness::WordCorrectness;

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

// TODO(.tasks/05-strategy.md): extract the strategy abstraction together with unconstrained
// guessing — immutable guess pool + shrinking candidate set weighted by an answer prior;
// tie-break equal scores in favor of words still in the candidate set.
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

    pub fn with_history(word_set: WordSet<N>, history: Vec<Guess<N>>) -> Self {
        let mut guesser = Self::new(word_set);
        for guess in history {
            guesser.push_guess(guess);
        }
        guesser
    }

    pub fn history(&self) -> &[Guess<N>] {
        &self.history
    }

    pub fn push_guess(&mut self, guess: Guess<N>) {
        let word = guess.word();
        let target_ordinal = guess.correctness().ordinal();
        self.word_set
            .retain(|w| WordCorrectness::correct(*w, word).ordinal() == target_ordinal);
        self.history.push(guess);
    }

    pub fn suggest(&self) -> Option<Word<N>> {
        if self.word_set.is_empty() {
            cold_path();
            return None;
        }

        let mut best: Option<(Word<N>, f64)> = None;
        for candidate in self.word_set.words() {
            let score = guess_entropy(&candidate, &self.word_set);
            log::trace!("{}: {}", candidate, score);
            match best {
                Some((best_word, best_score)) if score > best_score => {
                    log::debug!("{} > {}; {} > {}", candidate, best_word, score, best_score);
                    best = Some((candidate, score));
                }
                None => best = Some((candidate, score)),
                _ => {}
            }
        }
        best.map(|(word, _)| word)
    }
}

#[optimize(speed)]
fn guess_entropy<const N: usize>(guess: &Word<N>, word_set: &WordSet<N>) -> f64 {
    thread_local! {
        static PATTERN_PROBS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }
    PATTERN_PROBS.with(|probs| {
        let mut probs = probs.borrow_mut();
        if probs.is_empty() {
            *probs = vec![0.0; WordCorrectness::<N>::COUNT];
        } else {
            probs.fill(0.0);
        }
        for (word, prob) in word_set.word_probs() {
            log::trace!("{}: {}", word, prob);
            probs[WordCorrectness::correct(word, *guess).ordinal()] += prob;
        }
        -probs.iter().fold(
            0.0_f64,
            |acc, &p| if p > 0.0 { acc + p * p.log2() } else { acc },
        )
    })
}

#[cfg(test)]
mod benches {
    extern crate test;
    use std::collections::HashMap;
    use std::hint::black_box;
    use test::Bencher;

    use lex_data::{Word, WordSet};

    use super::*;

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
        let ws = WordSet::from_frequency_map(frequencies);
        Guesser::<5>::new(ws)
    }

    #[bench]
    fn guess_entropy_10_words(b: &mut Bencher) {
        let pairs = make_wordset(10);
        let guess = pairs[0].0;
        let frequencies: HashMap<Word<5>, u64> = pairs.iter().cloned().collect();
        let ws = WordSet::from_frequency_map(frequencies);
        b.iter(|| black_box(guess_entropy(&guess, &ws)));
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

    fn make_guesser(pairs: &[(&str, u64)]) -> Guesser<5> {
        let frequencies = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        let ws = WordSet::from_frequency_map(frequencies);
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
        let g = make_guesser(&[
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

    #[test]
    fn cumulative_filter_across_two_guesses() {
        // Verifies that push_guess applies each constraint immediately, so two pushes without
        // an intervening suggest() correctly narrow the candidate set by both constraints.
        let mut g = make_guesser(&[
            ("crane", 100),
            ("crave", 100),
            ("craze", 100),
            ("graze", 100),
            ("trace", 100),
        ]);
        g.push_guess(make_guess("crane", "crave"));
        g.push_guess(make_guess("craze", "crave"));
        assert_eq!(g.suggest().unwrap(), word("crave"));
    }

    proptest::proptest! {
        #[test]
        fn target_always_survives_filter(target in "[a-z]{5}", guess_str in "[a-z]{5}") {
            let target_word = Word::<5>::try_from(target.as_str()).unwrap();
            let guess_word = Word::<5>::try_from(guess_str.as_str()).unwrap();
            let feedback = WordCorrectness::correct(target_word, guess_word);

            let mut g = make_guesser(&[(target.as_str(), 100)]);
            g.push_guess(Guess::new(guess_word, feedback));

            // correct(target, guess) == feedback by construction -> target always passes its own filter
            proptest::prop_assert_eq!(g.suggest(), Some(target_word));
        }
    }
}
