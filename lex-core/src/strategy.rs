//! The Strategy, the Guess pool policy, and the Answer prior.

use std::collections::HashMap;
use std::sync::Arc;

use lex_data::{MAX_ALPHABET_SIZE, Word, WordSet};
use strum::{Display, EnumString, VariantArray, VariantNames};

use crate::correctness::{Code, LetterCounts, UNKNOWN_LETTER, pattern_ordinal};

/// The temperature the report's evaluation prior is always computed at, so runs with different
/// solver temperatures stay comparable.
pub const CANONICAL_TEMPERATURE: f64 = 5.0;

/// How a word's corpus frequency becomes its Answer prior weight. The weights are fixed per word
/// for a game; the prior is renormalized over the Candidate set as it shrinks, never re-ranked.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PriorShape {
    /// `f^(1/T)` at the Strategy's temperature.
    #[default]
    Power,
    /// A logistic of frequency rank over the Answer set, falling from near 1 above `cutoff` (a
    /// fraction of the set, in (0, 1]) to near 0 below it. The temperature has no effect.
    Sigmoid {
        /// Where the weight crosses one half, as a fraction of the Answer set's size.
        cutoff: f64,
    },
    /// `f^(1/T)` after lifting every frequency below the given percentile of the Answer set's
    /// frequencies up to that percentile's value.
    Floor {
        /// The percentile, in [0, 100), whose frequency is the lowest any word keeps.
        percentile: f64,
    },
}

/// Why a [`PriorShape`] is rejected.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PriorShapeError {
    /// A sigmoid cut-off outside (0, 1].
    #[error("invalid sigmoid cutoff: expected a number in (0, 1], got {0}")]
    Cutoff(f64),
    /// A floor percentile outside [0, 100).
    #[error("invalid floor percentile: expected a number in [0, 100), got {0}")]
    Percentile(f64),
    /// A shape name that is not `power`, `sigmoid:<cutoff>` or `floor:<percentile>`.
    #[error("unknown prior shape {0:?}: expected power, sigmoid:<cutoff> or floor:<percentile>")]
    Unknown(String),
}

/// Sigmoid width, as a fraction of the Answer set's size.
const SIGMOID_WIDTH: f64 = 0.1;

impl PriorShape {
    /// Checks the parameter is in range.
    ///
    /// # Errors
    ///
    /// If a sigmoid cut-off is outside (0, 1] or a floor percentile outside [0, 100).
    pub fn validate(self) -> Result<(), PriorShapeError> {
        match self {
            Self::Power => Ok(()),
            Self::Sigmoid { cutoff } if cutoff > 0.0 && cutoff <= 1.0 => Ok(()),
            Self::Sigmoid { cutoff } => Err(PriorShapeError::Cutoff(cutoff)),
            Self::Floor { percentile } if (0.0..100.0).contains(&percentile) => Ok(()),
            Self::Floor { percentile } => Err(PriorShapeError::Percentile(percentile)),
        }
    }

    /// Whether the Strategy's temperature enters the weights.
    const fn uses_temperature(self) -> bool {
        !matches!(self, Self::Sigmoid { .. })
    }

    /// The log of each pool word's weight before the temperature, given the pool's frequencies
    /// (`counts`, with their logs in `ln_freqs`) and the Answer set's frequencies.
    fn scores(self, counts: &[u64], ln_freqs: &[f64], mut answers: Vec<u64>) -> Vec<f64> {
        if answers.is_empty() {
            return ln_freqs.to_vec();
        }
        answers.sort_unstable();
        match self {
            Self::Power => ln_freqs.to_vec(),
            Self::Sigmoid { cutoff } => {
                let size = count_f64(answers.len());
                counts
                    .iter()
                    .map(|&c| {
                        // Words with equal frequency share the rank of the first of them.
                        let rank = answers.len() - answers.partition_point(|&a| a <= c);
                        let x = cutoff.mul_add(size, -count_f64(rank)) / (SIGMOID_WIDTH * size);
                        // ln(logistic(x)), stable for large negative x
                        -(-x).exp().ln_1p()
                    })
                    .collect()
            }
            Self::Floor { percentile } => {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "the percentile is in [0, 100), so the index is below the set size"
                )]
                let at = (percentile / 100.0 * count_f64(answers.len())).floor() as usize;
                let floor = ln_count(answers[at.min(answers.len() - 1)]);
                ln_freqs.iter().map(|&lf| lf.max(floor)).collect()
            }
        }
    }
}

/// Natural log of a frequency.
fn ln_count(count: u64) -> f64 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "ngram counts stay far below 2^53, so the float holds each count exactly"
    )]
    let f = count as f64;
    f.ln()
}

/// A word count as a float.
const fn count_f64(n: usize) -> f64 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "word counts stay far below 2^53, so the float holds each exactly"
    )]
    let f = n as f64;
    f
}

impl std::fmt::Display for PriorShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Power => write!(f, "power"),
            Self::Sigmoid { cutoff } => write!(f, "sigmoid:{cutoff}"),
            Self::Floor { percentile } => write!(f, "floor:{percentile}"),
        }
    }
}

impl std::str::FromStr for PriorShape {
    type Err = PriorShapeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let unknown = || PriorShapeError::Unknown(s.to_owned());
        let shape = match s.split_once(':') {
            None if s.eq_ignore_ascii_case("power") => Self::Power,
            Some((name, value)) => {
                let value: f64 = value.parse().map_err(|_| unknown())?;
                if name.eq_ignore_ascii_case("sigmoid") {
                    Self::Sigmoid { cutoff: value }
                } else if name.eq_ignore_ascii_case("floor") {
                    Self::Floor { percentile: value }
                } else {
                    return Err(unknown());
                }
            }
            None => return Err(unknown()),
        };
        shape.validate()?;
        Ok(shape)
    }
}

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
    /// Guesses a rollout compares at a mid-sized Candidate set: the top `rollout_k` by entropy
    /// score are each played out under the base policy and the cheapest in expected guesses wins.
    /// Zero turns rollout off.
    pub rollout_k: usize,
    /// How frequency becomes the Answer prior weight; [`PriorShape::Power`] is `f^(1/T)`.
    pub prior_shape: PriorShape,
}

impl Strategy {
    /// Unconstrained guessing with the temperature and win weight chosen by the sweep recorded in
    /// `benchmarks/experiments/strategy.md`.
    pub const DEFAULT: Self = Self {
        pool: GuessPool::All,
        temperature: CANONICAL_TEMPERATURE,
        win_weight: 1.0,
        endgame_threshold: 10,
        endgame_aversion: 0.0,
        rollout_k: 10,
        prior_shape: PriorShape::Power,
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
            rollout_k: 0,
            prior_shape: PriorShape::Power,
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
            rollout_k,
            prior_shape,
        } = *self;
        vec![
            ("pool", pool.to_string()),
            ("temperature", temperature.to_string()),
            ("win_weight", win_weight.to_string()),
            ("endgame_threshold", endgame_threshold.to_string()),
            ("endgame_aversion", endgame_aversion.to_string()),
            ("rollout_k", rollout_k.to_string()),
            ("prior_shape", prior_shape.to_string()),
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
        assert!(
            self.prior_shape.validate().is_ok(),
            "invalid prior shape {}",
            self.prior_shape
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
    /// Log of each word's prior weight before the temperature (its log-frequency under
    /// [`PriorShape::Power`]), parallel to `words`.
    scores: Vec<f64>,
    /// Whether the temperature divides `scores`; a shape that ignores it divides by 1.
    uses_temperature: bool,
    /// Number of words, as the pool-index type.
    count: u32,
}

impl<const N: usize> Pool<N> {
    /// # Panics
    ///
    /// If the Alphabet exceeds [`MAX_ALPHABET_SIZE`] (`DataDir::load` rejects such a Corpus), or
    /// the word set holds more words than a `u32` pool index addresses.
    pub(crate) fn new(word_set: &WordSet<N>) -> Arc<Self> {
        Self::shaped(word_set, &[], PriorShape::Power)
    }

    /// As [`new`](Self::new), weighting words by `shape` over the frequencies of `answers`, the
    /// set the prior covers. An answer outside the pool is ignored here.
    ///
    /// # Panics
    ///
    /// As [`new`](Self::new).
    pub(crate) fn shaped(
        word_set: &WordSet<N>,
        answers: &[Word<N>],
        shape: PriorShape,
    ) -> Arc<Self> {
        let mut alphabet: Vec<char> = word_set.alphabet().into_iter().collect();
        assert!(
            alphabet.len() <= MAX_ALPHABET_SIZE,
            "the Alphabet has {} letters, more than the {MAX_ALPHABET_SIZE} a byte can index",
            alphabet.len()
        );
        alphabet.sort_unstable();
        let letters: HashMap<char, u8> = alphabet.into_iter().zip(0..=u8::MAX).collect();
        let (words, counts): (Vec<_>, Vec<u64>) = word_set.word_frequencies().unzip();
        let ln_freqs: Vec<f64> = counts.iter().map(|&c| ln_count(c)).collect();
        let scores = if shape == PriorShape::Power {
            ln_freqs
        } else {
            let by_word: HashMap<Word<N>, u64> =
                words.iter().copied().zip(counts.iter().copied()).collect();
            let answer_counts = answers.iter().filter_map(|w| by_word.get(w).copied());
            shape.scores(&counts, &ln_freqs, answer_counts.collect())
        };
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
            scores,
            uses_temperature: shape.uses_temperature(),
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

    /// Number of letters in the Alphabet; every pool code is below it.
    pub(crate) fn alphabet_len(&self) -> usize {
        self.letters.len()
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

    /// Fills `out` with `(feedback ordinal, answer)` for every answer in `set` against `guess`,
    /// grouped by ordinal and ascending by index within a group.
    pub(crate) fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>) {
        let code = self.code(guess);
        let mut counts: LetterCounts = [0; 256];
        out.clear();
        out.extend(
            set.iter()
                .map(|&i| (pattern_ordinal(self.code(i), code, &mut counts), i)),
        );
        group_by_ordinal(out);
    }

    /// The log prior weight, before the temperature, of the word at pool index `index`.
    fn score(&self, index: u32) -> f64 {
        self.scores[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// The divisor of the scores: `temperature`, or 1 for a shape that ignores it.
    const fn divisor(&self, temperature: f64) -> f64 {
        if self.uses_temperature {
            temperature
        } else {
            1.0
        }
    }

    /// Unnormalized answer prior for every word: exp((ln f - max ln f) / T) over the whole pool.
    /// Ratios between words match [`prior`](Self::prior); unlike it, the scale does not depend on
    /// which words survive, so a sum over any subset is comparable across game positions.
    pub(crate) fn weights(&self, temperature: f64) -> Vec<f64> {
        let temperature = self.divisor(temperature);
        let max_lf = self
            .scores
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        self.scores
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
        let temperature = self.divisor(temperature);
        let max_lf = indices
            .iter()
            .map(|&i| self.score(i))
            .fold(f64::NEG_INFINITY, f64::max);
        let exp: Vec<f64> = indices
            .iter()
            .map(|&i| ((self.score(i) - max_lf) / temperature).exp())
            .collect();
        let sum: f64 = exp.iter().sum();
        exp.into_iter().map(|e| e / sum).collect()
    }
}

/// Groups `(ordinal, answer)` pairs by ordinal. The sort is stable, so a group stays ascending by
/// index. It is not generic over the word length, so the sort is compiled once.
fn group_by_ordinal(pairs: &mut [(u64, u32)]) {
    pairs.sort_by_key(|&(ordinal, _)| ordinal);
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
            rollout_k: 4,
            // a unit variant, so Debug prints no nested field names
            prior_shape: PriorShape::Power,
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
        assert_eq!(values, ["hard", "2.5", "0.125", "7", "0.5", "4", "power"]);
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
            Strategy {
                rollout_k: 4,
                ..base
            },
            Strategy {
                prior_shape: PriorShape::Sigmoid { cutoff: 0.25 },
                ..base
            },
            Strategy {
                prior_shape: PriorShape::Floor { percentile: 25.0 },
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

    fn ws(freqs: &[u64]) -> (WordSet<5>, Vec<Word<5>>) {
        let names = ["crane", "stare", "light", "nacre", "mound", "plumb"];
        let pairs: Vec<_> = names
            .iter()
            .zip(freqs)
            .map(|(&s, &f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        let words = pairs.iter().map(|&(w, _)| w).collect();
        (WordSet::from_pairs(pairs), words)
    }

    fn shaped(freqs: &[u64], shape: PriorShape, t: f64) -> Vec<f64> {
        let (set, answers) = ws(freqs);
        let pool = Pool::shaped(&set, &answers, shape);
        let all: Vec<u32> = pool.indices().collect();
        pool.prior(&all, t)
    }

    const FREQS: [u64; 6] = [600, 500, 500, 100, 20, 3];

    #[test]
    fn power_is_the_plain_pool_prior_bit_for_bit() {
        let (set, answers) = ws(&FREQS);
        let all: Vec<u32> = (0..6).collect();
        let plain = Pool::new(&set).prior(&all, 5.0);
        let shaped = Pool::shaped(&set, &answers, PriorShape::Power).prior(&all, 5.0);
        assert_eq!(plain, shaped);
        assert_eq!(
            Pool::new(&set).weights(5.0),
            Pool::shaped(&set, &answers, PriorShape::Power).weights(5.0)
        );
    }

    #[test]
    fn sigmoid_falls_with_rank_shares_ties_and_ignores_temperature() {
        let shape = PriorShape::Sigmoid { cutoff: 0.5 };
        let p = shaped(&FREQS, shape, 5.0);
        assert!(p.windows(2).all(|w| w[0] >= w[1]), "{p:?}");
        assert_eq!(
            p[1].to_bits(),
            p[2].to_bits(),
            "equal frequencies share a weight"
        );
        assert!(p[2] > p[3]);
        assert_eq!(p, shaped(&FREQS, shape, 1.0));
        assert_eq!(p, shaped(&FREQS, shape, 20.0));
    }

    #[test]
    fn floor_at_zero_is_power_and_flattens_the_tail_otherwise() {
        let power = shaped(&FREQS, PriorShape::Power, 5.0);
        assert_eq!(
            shaped(&FREQS, PriorShape::Floor { percentile: 0.0 }, 5.0),
            power
        );
        let floored = shaped(&FREQS, PriorShape::Floor { percentile: 50.0 }, 5.0);
        // The words at or below the 50th percentile (index 3 of the ascending counts: 500) tie.
        assert_eq!(floored[3].to_bits(), floored[4].to_bits());
        assert_eq!(floored[4].to_bits(), floored[5].to_bits());
        assert_eq!(floored[1].to_bits(), floored[3].to_bits());
        assert!(floored[0] > floored[1]);
        assert!(floored[5] > power[5]);
    }

    #[test]
    fn every_shape_sums_to_one_over_a_candidate_set() {
        let (set, answers) = ws(&FREQS);
        for shape in [
            PriorShape::Power,
            PriorShape::Sigmoid { cutoff: 0.1 },
            PriorShape::Sigmoid { cutoff: 1.0 },
            PriorShape::Floor { percentile: 75.0 },
        ] {
            let pool = Pool::shaped(&set, &answers, shape);
            for subset in [&[0, 2, 5][..], &[1, 3], &[4]] {
                let sum: f64 = pool.prior(subset, 5.0).iter().sum();
                assert!((sum - 1.0).abs() < 1e-12, "{shape} {subset:?} {sum}");
            }
        }
    }

    #[test]
    fn ranks_come_from_the_answer_set_not_the_pool() {
        let (set, answers) = ws(&FREQS);
        let shape = PriorShape::Sigmoid { cutoff: 0.5 };
        let all: Vec<u32> = (0..6).collect();
        let full = Pool::shaped(&set, &answers, shape).prior(&all, 5.0);
        let narrow = Pool::shaped(&set, &answers[3..], shape).prior(&all, 5.0);
        assert_ne!(full, narrow);
    }

    #[test]
    fn shapes_parse_display_and_validate() {
        for text in ["power", "sigmoid:0.25", "floor:50", "sigmoid:1"] {
            assert_eq!(text.parse::<PriorShape>().unwrap().to_string(), text);
        }
        for bad in [
            "sigmoid:0",
            "sigmoid:1.5",
            "sigmoid:nan",
            "floor:100",
            "floor:-1",
            "floor",
            "sigmoid",
            "power:1",
            "other:1",
            "",
        ] {
            assert!(bad.parse::<PriorShape>().is_err(), "{bad}");
        }
    }
}
