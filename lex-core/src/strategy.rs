//! The Strategy, the Guess pool policy, and the Answer prior.

use std::collections::HashMap;
use std::sync::Arc;

use lex_data::{MAX_ALPHABET_SIZE, Word, WordSet};
use strum::{Display, EnumString, VariantArray, VariantNames};

use crate::correctness::{Code, UNKNOWN_LETTER};

/// The temperature the report's evaluation prior is always computed at, so runs with different
/// solver temperatures stay comparable.
pub const CANONICAL_TEMPERATURE: f64 = 5.0;

/// Which words the solver may play as a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, VariantNames, VariantArray)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
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
    /// The Guess pool policy.
    pub pool: GuessPool,
    /// Answer prior temperature: `p_i` is proportional to `f_i^(1/T)`. Must be positive and finite.
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
    /// Unconstrained guessing with the temperature and win weight chosen by the sweep recorded in
    /// `benchmarks/experiments/strategy.md`.
    pub const DEFAULT: Self = Self {
        pool: GuessPool::All,
        temperature: CANONICAL_TEMPERATURE,
        win_weight: 1.0,
        endgame_threshold: 0,
        endgame_aversion: 0.0,
    };

    /// The original solver: ultrahard guesses (on a Corpus run, only the candidates), scored by
    /// entropy alone.
    #[must_use]
    pub const fn ultrahard() -> Self {
        Self {
            pool: GuessPool::Ultrahard,
            temperature: CANONICAL_TEMPERATURE,
            win_weight: 0.0,
            endgame_threshold: 0,
            endgame_aversion: 0.0,
        }
    }

    /// Every field as an ordered (name, value) pair: what an Opening book is keyed on besides
    /// the pool and the solver source. Destructuring without `..` makes a new field a compile
    /// error here until it is listed, and floats print in their shortest round-tripping form.
    #[must_use]
    pub fn key_fields(&self) -> Vec<(&'static str, String)> {
        let Self {
            pool,
            temperature,
            win_weight,
            endgame_threshold,
            endgame_aversion,
        } = *self;
        vec![
            ("pool", pool.to_string()),
            ("temperature", temperature.to_string()),
            ("win_weight", win_weight.to_string()),
            ("endgame_threshold", endgame_threshold.to_string()),
            ("endgame_aversion", endgame_aversion.to_string()),
        ]
    }

    /// Asserts the temperature is positive and finite and the win weight non-negative and finite.
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

impl Default for Strategy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The immutable guess pool: every loaded word in frequency-descending order with its
/// log-frequency. Candidate sets index into it.
#[derive(Debug)]
pub struct Pool<const N: usize> {
    /// Every pool word, in frequency-descending order.
    words: Vec<Word<N>>,
    /// Each word's letters as indices into the Alphabet, parallel to `words`.
    codes: Vec<Code<N>>,
    /// Alphabet letter to its index.
    letters: HashMap<char, u8>,
    /// Natural log of each word's frequency, parallel to `words`.
    ln_freqs: Vec<f64>,
    /// Number of words, as the pool-index type.
    count: u32,
}

impl<const N: usize> Pool<N> {
    /// # Panics
    ///
    /// If the Alphabet exceeds [`MAX_ALPHABET_SIZE`] (`DataDir::load` rejects such a Corpus), or
    /// the word set holds more words than a `u32` pool index addresses.
    pub(crate) fn new(word_set: &WordSet<N>) -> Arc<Self> {
        let mut alphabet: Vec<char> = word_set.alphabet().into_iter().collect();
        assert!(
            alphabet.len() <= MAX_ALPHABET_SIZE,
            "the Alphabet has {} letters, more than the {MAX_ALPHABET_SIZE} a byte can index",
            alphabet.len()
        );
        alphabet.sort_unstable();
        let letters: HashMap<char, u8> = alphabet.into_iter().zip(0..=u8::MAX).collect();
        let (words, ln_freqs): (Vec<_>, _) = word_set
            .word_frequencies()
            .map(|(word, freq)| {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "ngram counts stay far below 2^53, so the float holds each count exactly"
                )]
                let freq = freq as f64;
                (word, freq.ln())
            })
            .unzip();
        let count = u32::try_from(words.len());
        assert!(
            count.is_ok(),
            "the word set holds {} words, more than a u32 pool index addresses",
            words.len()
        );
        let codes = words.iter().map(|w| encode(&letters, w)).collect();
        Arc::new(Self {
            words,
            codes,
            letters,
            ln_freqs,
            count: count.unwrap_or(u32::MAX),
        })
    }

    /// The pool word's letters as Alphabet indices.
    pub(crate) fn code(&self, index: u32) -> &Code<N> {
        &self.codes[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// Any word's letters as Alphabet indices; a letter outside the Alphabet becomes
    /// [`UNKNOWN_LETTER`], which no pool word holds.
    pub(crate) fn encode(&self, word: &Word<N>) -> Code<N> {
        encode(&self.letters, word)
    }

    /// Number of words in the pool.
    pub(crate) const fn len(&self) -> usize {
        self.words.len()
    }

    /// The word at pool index `index`.
    pub(crate) fn word(&self, index: u32) -> Word<N> {
        self.words[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// Every pool index, ascending.
    pub(crate) const fn indices(&self) -> std::ops::Range<u32> {
        0..self.count
    }

    /// Natural log of the frequency of the word at pool index `index`.
    fn ln_freq(&self, index: u32) -> f64 {
        self.ln_freqs[usize::try_from(index).unwrap_or(usize::MAX)]
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

    /// The answer prior over `indices`, renormalized to sum to 1: `p_i` is proportional to
    /// exp((ln `f_i` - max ln f) / T) over just those words.
    pub(crate) fn prior(&self, indices: &[u32], temperature: f64) -> Vec<f64> {
        if indices.is_empty() {
            return Vec::new();
        }
        let max_lf = indices
            .iter()
            .map(|&i| self.ln_freq(i))
            .fold(f64::NEG_INFINITY, f64::max);
        let exp: Vec<f64> = indices
            .iter()
            .map(|&i| ((self.ln_freq(i) - max_lf) / temperature).exp())
            .collect();
        let sum: f64 = exp.iter().sum();
        exp.into_iter().map(|e| e / sum).collect()
    }
}

/// `word`'s letters as Alphabet indices, [`UNKNOWN_LETTER`] for any outside it.
fn encode<const N: usize>(letters: &HashMap<char, u8>, word: &Word<N>) -> Code<N> {
    std::array::from_fn(|i| letters.get(&word[i]).copied().unwrap_or(UNKNOWN_LETTER))
}

/// The answer prior over a whole word set at `temperature`, in frequency-descending order.
///
/// Raw-frequency softmax collapses to one-hot on the top word because ngrams counts span many
/// orders of magnitude; log-space input with temperature above 1 flattens the distribution
/// toward uniform without discarding rank information.
#[must_use]
pub fn answer_prior<const N: usize>(
    word_set: &WordSet<N>,
    temperature: f64,
) -> Vec<(Word<N>, f64)> {
    let pool = Pool::new(word_set);
    let all: Vec<u32> = pool.indices().collect();
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
    fn the_key_lists_every_strategy_field_in_declaration_order() {
        let strategy = Strategy {
            pool: GuessPool::Hard,
            temperature: 2.5,
            win_weight: 0.125,
            endgame_threshold: 7,
            endgame_aversion: 0.5,
        };
        // Debug prints each field by name, so it is a source of field names that does not
        // come from `key_fields` itself.
        let declared: Vec<String> = format!("{strategy:#?}")
            .lines()
            .filter_map(|l| l.trim().split_once(": "))
            .map(|(name, _)| name.to_owned())
            .collect();
        let keyed: Vec<&str> = strategy.key_fields().iter().map(|&(n, _)| n).collect();
        assert_eq!(keyed, declared);
        let values: Vec<String> = strategy.key_fields().into_iter().map(|(_, v)| v).collect();
        assert_eq!(values, ["hard", "2.5", "0.125", "7", "0.5"]);
    }

    #[test]
    fn changing_any_field_changes_the_key() {
        let base = Strategy::DEFAULT;
        let variants = [
            Strategy {
                pool: GuessPool::Hard,
                ..base
            },
            Strategy {
                temperature: 4.0,
                ..base
            },
            Strategy {
                win_weight: 0.5,
                ..base
            },
            Strategy {
                endgame_threshold: 3,
                ..base
            },
            Strategy {
                endgame_aversion: 0.25,
                ..base
            },
        ];
        for variant in variants {
            assert_ne!(variant.key_fields(), base.key_fields(), "{variant:?}");
        }
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
