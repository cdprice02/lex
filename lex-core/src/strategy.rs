use std::collections::HashMap;
use std::sync::Arc;

use lex_data::{MAX_ALPHABET_SIZE, Word, WordSet};

use crate::correctness::{Code, UNKNOWN_LETTER};

/// The temperature the report's evaluation prior is always computed at, so runs with different
/// solver temperatures stay comparable.
pub const CANONICAL_TEMPERATURE: f64 = 5.0;

/// Which words the solver may play as a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessPool {
    /// Every word in the loaded set, including ones already ruled out as the answer.
    All,
    /// Words the official Wordle hard-mode rule allows: every earlier green stays in place, and
    /// every earlier green or yellow letter is used again at least as many times.
    Hard,
    /// Words consistent with every feedback so far, as if each could be the answer. On a Corpus
    /// run these are exactly the candidates; on a Profile they are a superset of the Answers left.
    Ultrahard,
}

/// The solver's tunable choices. Fixed for the life of a [`Guesser`](crate::Guesser): the decision
/// tree memoizes suggestions, so a different strategy needs a different guesser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strategy {
    pub pool: GuessPool,
    /// Answer prior temperature: p_i is proportional to f_i^(1/T). Must be positive and finite.
    pub temperature: f64,
    /// Bonus, in bits, per unit of probability that the guess is the answer. Must be finite and
    /// non-negative; zero scores purely by entropy.
    pub win_weight: f64,
    /// Candidate-set size at or below which the solver searches the game tree exactly instead of
    /// scoring by entropy; zero turns the endgame off.
    pub endgame_threshold: usize,
    /// Risk aversion of the endgame search, as the log of the per-guess growth factor `a`: it
    /// minimizes the prior-weighted mean of `a^guesses`, so a long game costs more than its
    /// length alone. Zero minimizes plain expected guesses. Must be finite and non-negative.
    pub endgame_aversion: f64,
}

impl Strategy {
    /// The original solver: ultrahard guesses (on a Corpus run, only the candidates), scored by
    /// entropy alone.
    pub const fn ultrahard() -> Self {
        Self {
            pool: GuessPool::Ultrahard,
            temperature: CANONICAL_TEMPERATURE,
            win_weight: 0.0,
            endgame_threshold: 0,
            endgame_aversion: 0.0,
        }
    }

    pub(crate) fn assert_valid(&self) {
        assert!(
            self.temperature.is_finite() && self.temperature > 0.0,
            "temperature must be positive and finite, got {}",
            self.temperature
        );
        assert!(
            self.win_weight.is_finite() && self.win_weight >= 0.0,
            "win_weight must be finite and non-negative, got {}",
            self.win_weight
        );
        assert!(
            self.endgame_aversion.is_finite() && self.endgame_aversion >= 0.0,
            "endgame_aversion must be finite and non-negative, got {}",
            self.endgame_aversion
        );
    }
}

/// Unconstrained guessing with the temperature and win weight chosen by the sweep recorded in
/// `benchmarks/experiments/strategy.md`.
impl Default for Strategy {
    fn default() -> Self {
        Self {
            pool: GuessPool::All,
            temperature: CANONICAL_TEMPERATURE,
            win_weight: 1.0,
            endgame_threshold: 0,
            endgame_aversion: 0.0,
        }
    }
}

/// The immutable guess pool: every loaded word in frequency-descending order with its
/// log-frequency. Candidate sets index into it.
#[derive(Debug)]
pub(crate) struct Pool<const N: usize> {
    words: Vec<Word<N>>,
    /// Each word's letters as indices into the Alphabet, parallel to `words`.
    codes: Vec<Code<N>>,
    letters: HashMap<char, u8>,
    ln_freqs: Vec<f64>,
}

impl<const N: usize> Pool<N> {
    /// # Panics
    ///
    /// If the Alphabet exceeds [`MAX_ALPHABET_SIZE`]: `DataDir::load` rejects such a Corpus.
    pub(crate) fn new(word_set: &WordSet<N>) -> Arc<Self> {
        let mut alphabet: Vec<char> = word_set.alphabet().into_iter().collect();
        assert!(
            alphabet.len() <= MAX_ALPHABET_SIZE,
            "the Alphabet has {} letters, more than the {MAX_ALPHABET_SIZE} a byte can index",
            alphabet.len()
        );
        alphabet.sort_unstable();
        let letters: HashMap<char, u8> = alphabet
            .into_iter()
            .enumerate()
            .map(|(i, c)| (c, i as u8))
            .collect();
        let (words, ln_freqs): (Vec<_>, _) = word_set
            .word_frequencies()
            .map(|(word, freq)| (word, (freq as f64).ln()))
            .unzip();
        let codes = words.iter().map(|w| encode(&letters, w)).collect();
        Arc::new(Self {
            words,
            codes,
            letters,
            ln_freqs,
        })
    }

    /// The pool word's letters as Alphabet indices.
    pub(crate) fn code(&self, index: u32) -> &Code<N> {
        &self.codes[index as usize]
    }

    /// Any word's letters as Alphabet indices; a letter outside the Alphabet becomes
    /// [`UNKNOWN_LETTER`], which no pool word holds.
    pub(crate) fn encode(&self, word: &Word<N>) -> Code<N> {
        encode(&self.letters, word)
    }

    pub(crate) fn len(&self) -> usize {
        self.words.len()
    }

    pub(crate) fn word(&self, index: u32) -> Word<N> {
        self.words[index as usize]
    }

    /// Unnormalized answer prior for every word: exp((ln f - max ln f) / T) over the whole pool.
    /// Ratios between words match [`prior`](Self::prior); unlike it, the scale does not depend on
    /// which words survive, so a sum over any subset is comparable across game positions.
    pub(crate) fn weights(&self, temperature: f64) -> Vec<f64> {
        let max_lf = self
            .ln_freqs
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        self.ln_freqs
            .iter()
            .map(|&lf| ((lf - max_lf) / temperature).exp().max(f64::MIN_POSITIVE))
            .collect()
    }

    /// The answer prior over `indices`, renormalized to sum to 1: p_i is proportional to
    /// exp((ln f_i - max ln f) / T) over just those words.
    pub(crate) fn prior(&self, indices: &[u32], temperature: f64) -> Vec<f64> {
        if indices.is_empty() {
            return Vec::new();
        }
        let max_lf = indices
            .iter()
            .map(|&i| self.ln_freqs[i as usize])
            .fold(f64::NEG_INFINITY, f64::max);
        let exp: Vec<f64> = indices
            .iter()
            .map(|&i| ((self.ln_freqs[i as usize] - max_lf) / temperature).exp())
            .collect();
        let sum: f64 = exp.iter().sum();
        exp.into_iter().map(|e| e / sum).collect()
    }
}

fn encode<const N: usize>(letters: &HashMap<char, u8>, word: &Word<N>) -> Code<N> {
    std::array::from_fn(|i| letters.get(&word[i]).copied().unwrap_or(UNKNOWN_LETTER))
}

/// The answer prior over a whole word set at `temperature`, in frequency-descending order.
/// Raw-frequency softmax collapses to one-hot on the top word because ngrams counts span many
/// orders of magnitude; log-space input with temperature above 1 flattens the distribution
/// toward uniform without discarding rank information.
pub fn answer_prior<const N: usize>(
    word_set: &WordSet<N>,
    temperature: f64,
) -> Vec<(Word<N>, f64)> {
    let pool = Pool::new(word_set);
    let all: Vec<u32> = (0..pool.len() as u32).collect();
    let probs = pool.prior(&all, temperature);
    pool.words.iter().copied().zip(probs).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(pairs: &[(&str, u64)]) -> WordSet<5> {
        WordSet::from_pairs(
            pairs
                .iter()
                .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
                .collect(),
        )
    }

    #[test]
    fn prior_sums_to_one_and_follows_frequency() {
        let prior = answer_prior(&set(&[("crane", 100), ("stare", 200), ("light", 300)]), 5.0);
        let sum: f64 = prior.iter().map(|&(_, p)| p).sum();
        assert!((sum - 1.0).abs() < 1e-12, "prior sums to {sum}");
        assert!(prior.windows(2).all(|w| w[0].1 >= w[1].1));
        assert_eq!(prior[0].0, Word::<5>::try_from("light").unwrap());
    }

    #[test]
    fn higher_temperature_flattens_the_prior() {
        let ws = set(&[("crane", 1), ("stare", 1_000_000)]);
        let spread = |t| {
            let p = answer_prior(&ws, t);
            p[0].1 - p[1].1
        };
        assert!(spread(20.0) < spread(1.0));
    }

    #[test]
    fn codes_index_the_alphabet_and_unknown_letters_get_the_sentinel() {
        let pool = Pool::new(&set(&[("crane", 300), ("nacre", 200)]));
        // alphabet in sorted order: a c e n r
        assert_eq!(pool.code(0), &[1, 4, 0, 3, 2]);
        assert_eq!(
            pool.encode(&Word::<5>::try_from("zebra").unwrap()),
            [UNKNOWN_LETTER, 2, UNKNOWN_LETTER, 4, 0]
        );
    }

    #[test]
    fn renormalizes_over_the_surviving_subset() {
        let pool = Pool::new(&set(&[("crane", 300), ("stare", 200), ("light", 100)]));
        let probs = pool.prior(&[1, 2], 5.0);
        assert_eq!(probs.len(), 2);
        assert!((probs.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "temperature")]
    fn rejects_zero_temperature() {
        Strategy {
            temperature: 0.0,
            ..Strategy::default()
        }
        .assert_valid();
    }
}
