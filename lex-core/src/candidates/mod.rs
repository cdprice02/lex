//! The Candidate set and its entropy scoring.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::hint::cold_path;
use std::sync::Arc;

use lex_data::Word;

use crate::allowed::Allowed;
#[cfg(any(test, feature = "bench"))]
use crate::correctness::Code;
use crate::correctness::{LetterCounts, WordCorrectness, pattern_ordinal};
use crate::guesser::Guess;
use crate::strategy::{Pool, Strategy};
mod counters;

use crate::transposed::{Columns, Plan};
use counters::{Scratch, guess_entropy_columns};

/// A scored guess: how much it is expected to teach, and whether it could itself be the answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion<const N: usize> {
    /// The guess itself.
    pub word: Word<N>,
    /// Expected information gain under the answer prior, in bits.
    pub entropy_bits: f64,
    /// Probability that this word is the answer; zero for a guess already ruled out.
    pub p_answer: f64,
    /// Whether the word is still consistent with every guess played.
    pub in_candidates: bool,
}

impl<const N: usize> Suggestion<N> {
    /// The quantity the solver maximizes: entropy plus the bonus for a chance of winning now.
    #[must_use]
    pub const fn score(&self, win_weight: f64) -> f64 {
        win_weight.mul_add(self.p_answer, self.entropy_bits)
    }
}

/// A guess scored against a Candidate set, by its pool index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// The guess's index in the Guess pool.
    pub index: u32,
    /// Expected information gain under the answer prior, in bits.
    pub entropy_bits: f64,
    /// Probability that the guess is the answer; zero for a guess already ruled out.
    pub p_answer: f64,
    /// Whether the guess is still consistent with every guess played.
    pub in_candidates: bool,
}

impl Scored {
    /// The quantity the solver maximizes, as [`Suggestion::score`].
    pub(crate) const fn score(&self, win_weight: f64) -> f64 {
        win_weight.mul_add(self.p_answer, self.entropy_bits)
    }

    /// The public view of this guess.
    pub(crate) fn suggestion<const N: usize>(&self, pool: &Pool<N>) -> Suggestion<N> {
        Suggestion {
            word: pool.word(self.index),
            entropy_bits: self.entropy_bits,
            p_answer: self.p_answer,
            in_candidates: self.in_candidates,
        }
    }
}

/// The words still consistent with every guess played so far, as indices into the immutable
/// guess pool, and the choice of the next guess under a [`Strategy`]. Holds no history and caches
/// nothing: a [`DecisionTree`] node owns one and memoizes its suggestion.
///
/// [`DecisionTree`]: crate::tree::DecisionTree
#[derive(Debug, Clone)]
pub struct Candidates<const N: usize> {
    /// The Guess pool the indices point into.
    pool: Arc<Pool<N>>,
    /// Ascending, so frequency-descending.
    indices: Vec<u32>,
}

impl<const N: usize> Candidates<N> {
    /// Starts from exactly `answers`, as indices into the pool, so a Profile's Answer list and a
    /// Corpus run's every-word list take the same path.
    ///
    /// # Panics
    ///
    /// If an answer is not in the pool: `lex-data` guarantees the Answer list is a subset.
    #[expect(
        clippy::panic,
        reason = "an answer outside the pool breaks the lex-data subset guarantee; there is no valid game to play"
    )]
    pub(crate) fn new(pool: Arc<Pool<N>>, answers: &[Word<N>]) -> Self {
        let by_word: HashMap<Word<N>, u32> = pool.indices().map(|i| (pool.word(i), i)).collect();
        let mut indices: Vec<u32> = answers
            .iter()
            .map(|w| {
                *by_word
                    .get(w)
                    .unwrap_or_else(|| panic!("answer {w} is not in the guess pool"))
            })
            .collect();
        indices.sort_unstable();
        Self { pool, indices }
    }

    /// A Candidate set of exactly `indices`, ascending pool indices already filtered by the caller.
    pub(crate) const fn from_indices(pool: Arc<Pool<N>>, indices: Vec<u32>) -> Self {
        Self { pool, indices }
    }

    /// Every pool word is a candidate.
    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn whole_pool(pool: Arc<Pool<N>>) -> Self {
        let indices = pool.indices().collect();
        Self { pool, indices }
    }

    /// The candidates that remain after `guess` is played.
    pub(crate) fn filtered(&self, guess: &Guess<N>) -> Self {
        let code = self.pool.encode(&guess.word());
        let target_ordinal = guess.correctness().ordinal();
        let mut counts: LetterCounts = [0; 256];
        let indices = self
            .indices
            .iter()
            .copied()
            .filter(|&i| pattern_ordinal(self.pool.code(i), &code, &mut counts) == target_ordinal)
            .collect();
        Self {
            pool: Arc::clone(&self.pool),
            indices,
        }
    }

    /// A shared handle to the Guess pool.
    pub(crate) fn pool(&self) -> Arc<Pool<N>> {
        Arc::clone(&self.pool)
    }

    /// The Candidate set as ascending Guess pool indices.
    pub(crate) fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Size of the Candidate set.
    pub(crate) const fn len(&self) -> usize {
        self.indices.len()
    }

    /// Calls `f(pool index, entropy, p_answer, in_candidates)` for every guess `allowed` permits,
    /// in frequency-descending order.
    fn for_each_guess(
        &self,
        allowed: &Allowed,
        temperature: f64,
        mut f: impl FnMut(u32, f64, f64, bool),
    ) {
        let probs = self.pool.prior(&self.indices, temperature);
        let columns = Columns::gather(
            self.indices.iter().map(|&i| self.pool.code(i)),
            self.pool.alphabet_len(),
        );
        let mut scratch = Scratch::take::<N>();
        let mut scan = |guesses: &mut dyn Iterator<Item = u32>| {
            let mut next = 0;
            for i in guesses {
                let (p_answer, in_candidates) = match self.indices.get(next) {
                    Some(&c) if c == i => {
                        next += 1;
                        (probs[next - 1], true)
                    }
                    _ => (0.0, false),
                };
                f(
                    i,
                    guess_entropy_columns(
                        &Plan::new(self.pool.code(i)),
                        &columns,
                        &probs,
                        &mut scratch,
                    ),
                    p_answer,
                    in_candidates,
                );
            }
        };
        match allowed {
            Allowed::Candidates => scan(&mut self.indices.iter().copied()),
            Allowed::Whole => scan(&mut self.pool.indices()),
            Allowed::List(list) => scan(&mut list.iter().copied()),
        }
        scratch.give_back();
    }

    /// The `k` best guesses by score, descending. Equal scores prefer words still in the
    /// candidate set, then frequency order, so the first element agrees with
    /// `suggest_index`. Only the `k` kept are sorted.
    pub(crate) fn suggest_top_k(
        &self,
        strategy: &Strategy,
        allowed: &Allowed,
        k: usize,
    ) -> Vec<Scored> {
        let mut scored = Vec::new();
        self.for_each_guess(
            allowed,
            strategy.temperature,
            |index, entropy_bits, p_answer, in_candidates| {
                scored.push(Scored {
                    index,
                    entropy_bits,
                    p_answer,
                    in_candidates,
                });
            },
        );
        keep_best(&mut scored, strategy.win_weight, k);
        scored
    }

    /// The guess with the best score among those `allowed` permits, preferring a Candidate on ties; `None` when the set is empty.
    ///
    /// Tests and benches only: the solver goes through [`suggest_index`](Self::suggest_index).
    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn suggest(&self, strategy: &Strategy, allowed: &Allowed) -> Option<Word<N>> {
        self.suggest_index(strategy, allowed)
            .map(|index| self.pool.word(index))
    }

    /// The best guess as a pool index; `suggest` wraps it.
    pub(crate) fn suggest_index(&self, strategy: &Strategy, allowed: &Allowed) -> Option<u32> {
        match self.indices.as_slice() {
            [] => {
                cold_path();
                return None;
            }
            // every guess scores zero entropy against a single candidate, so it is the answer
            &[only] => return Some(only),
            _ => {}
        }

        // (index, score, in_candidates)
        let mut best: Option<(u32, f64, bool)> = None;
        self.for_each_guess(
            allowed,
            strategy.temperature,
            |i, entropy_bits, p_answer, in_candidates| {
                let score = strategy.win_weight.mul_add(p_answer, entropy_bits);
                log::trace!("{}: {}", self.pool.word(i), score);
                match best {
                    Some((best_i, best_score, best_in)) => {
                        let beats = match cmp_scores(score, best_score) {
                            Ordering::Greater => true,
                            Ordering::Equal => in_candidates && !best_in,
                            Ordering::Less => false,
                        };
                        if beats {
                            log::debug!(
                                "{} > {}; {} > {}",
                                self.pool.word(i),
                                self.pool.word(best_i),
                                score,
                                best_score
                            );
                            best = Some((i, score, in_candidates));
                        }
                    }
                    None => best = Some((i, score, in_candidates)),
                }
            },
        );
        best.map(|(i, _, _)| i)
    }
}

/// Keeps the `k` best of `scored` by score, descending: equal scores prefer a candidate, then the
/// lower pool index, so the order is total and the unstable selection deterministic. Only the `k`
/// kept are sorted. It is not generic over the word length, so the sorts are compiled once.
fn keep_best(scored: &mut Vec<Scored>, win_weight: f64, k: usize) {
    let better = |a: &Scored, b: &Scored| {
        cmp_scores(b.score(win_weight), a.score(win_weight))
            .then(b.in_candidates.cmp(&a.in_candidates))
            .then(a.index.cmp(&b.index))
    };
    if k < scored.len() {
        if let Some(last) = k.checked_sub(1) {
            scored.select_nth_unstable_by(last, better);
        }
        scored.truncate(k);
    }
    scored.sort_unstable_by(better);
}

/// Orders two finite scores. Adding `0.0` first folds `-0.0` into `0.0`, so a guess scoring zero
/// entropy ties with another zero whichever sign its sum landed on.
fn cmp_scores(a: f64, b: f64) -> Ordering {
    (a + 0.0).total_cmp(&(b + 0.0))
}

#[cfg(test)]
mod tests;

#[cfg(feature = "bench")]
/// A reproducible pool of `count` distinct words over a..z, frequency-descending, so every
/// bench scores against letter overlap like a real corpus rather than disjoint words.
pub fn synthetic_pool<const N: usize>(count: usize) -> Arc<Pool<N>> {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut seen = std::collections::HashSet::new();
    let mut pairs = Vec::with_capacity(count);
    while pairs.len() < count {
        let chars: [char; N] = std::array::from_fn(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            crate::guesser::letter((state >> 33) % 26)
        });
        if seen.insert(chars) {
            pairs.push((
                Word::<N>::from(chars),
                u64::try_from(count - pairs.len()).unwrap_or(1) * 10,
            ));
        }
    }
    Pool::new(&lex_data::WordSet::from_pairs(pairs))
}

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches;
