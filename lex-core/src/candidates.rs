use std::cell::RefCell;
use std::hint::cold_path;

use lex_data::{Word, WordSet};

use crate::correctness::WordCorrectness;
use crate::guesser::Guess;

/// The words still consistent with every guess played so far, and the entropy-based choice of
/// the next guess among them. Holds no history and caches nothing: a [`DecisionTree`] node owns
/// one and memoizes its [`suggest`](Self::suggest).
///
/// [`DecisionTree`]: crate::tree::DecisionTree
#[derive(Debug, Clone)]
pub(crate) struct Candidates<const N: usize> {
    word_set: WordSet<N>,
}

impl<const N: usize> Candidates<N> {
    pub(crate) fn new(word_set: WordSet<N>) -> Self {
        Self { word_set }
    }

    /// The candidates that remain after `guess` is played.
    pub(crate) fn filtered(&self, guess: &Guess<N>) -> Self {
        let word = guess.word();
        let target_ordinal = guess.correctness().ordinal();
        let mut word_set = self.word_set.clone();
        word_set.retain(|w| WordCorrectness::correct(*w, word).ordinal() == target_ordinal);
        Self { word_set }
    }

    pub(crate) fn len(&self) -> usize {
        self.word_set.len()
    }

    /// The `k` most informative candidates with their entropy scores (bits), sorted descending.
    /// Ties keep frequency-descending order, so the first element agrees with
    /// [`suggest`](Self::suggest).
    pub(crate) fn suggest_top_k(&self, k: usize) -> Vec<(Word<N>, f64)> {
        let mut scored: Vec<(Word<N>, f64)> = self
            .word_set
            .words()
            .map(|candidate| (candidate, guess_entropy(&candidate, &self.word_set)))
            .collect();
        // stable sort: equal scores keep frequency-descending iteration order
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("entropy scores are finite"));
        scored.truncate(k);
        scored
    }

    pub(crate) fn suggest(&self) -> Option<Word<N>> {
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
        // 0.0 - acc rather than -acc: a zero sum yields +0.0, not -0.0
        0.0 - probs.iter().fold(
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

    use super::*;

    #[bench]
    fn guess_entropy_10_words(b: &mut Bencher) {
        let pairs: Vec<(Word<5>, u64)> = (0..10)
            .map(|i| {
                let s: String = [char::from(b'a' + i as u8); 5].iter().collect();
                (Word::<5>::try_from(s.as_str()).unwrap(), 10 - i as u64)
            })
            .collect();
        let guess = pairs[0].0;
        let frequencies: HashMap<Word<5>, u64> = pairs.iter().cloned().collect();
        let ws = WordSet::from_frequency_map(frequencies);
        b.iter(|| black_box(guess_entropy(&guess, &ws)));
    }
}
