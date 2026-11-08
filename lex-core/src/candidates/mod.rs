//! The Candidate set and its entropy scoring.

use std::cell::RefCell;
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
use crate::transposed::{BLOCK, Columns, MAX_U32_LENGTH, Ordinal, Plan};

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

/// Largest bucket array the dense counter may allocate per thread. Working memory like this sits
/// outside the 1 GB corpus cap; the bound makes the dense/sorted cut-over a memory rule rather
/// than a list of word lengths.
const DENSE_BOUND_BYTES: u64 = 16 * 1024 * 1024;

/// Bytes one dense bucket takes.
const BUCKET_BYTES: u64 = 8;
const _: () = assert!(size_of::<f64>() == 8, "a dense bucket is one f64");

/// The per-thread working memory of an entropy scan, taken once per scan and reused for every
/// guess in it.
#[derive(Default)]
struct Scratch {
    /// Probability mass per feedback ordinal. Every bucket is zero between guesses; `touched`
    /// lists the ones a guess dirtied, so neither the reset nor the sum walks all 3^N buckets.
    buckets: Vec<f64>,
    /// Ordinals of the dirtied buckets.
    touched: Vec<u64>,
    /// `(ordinal, probability)` pairs for the sorting counter.
    pairs: Vec<(u64, f64)>,
}

thread_local! {
    /// The scratch between scans. One static serves every monomorphization, so it grows to the
    /// largest N seen.
    static SCRATCH: RefCell<Scratch> = const { RefCell::new(Scratch {
        buckets: Vec::new(),
        touched: Vec::new(),
        pairs: Vec::new(),
    }) };
}

impl Scratch {
    /// This thread's scratch, sized for words of length `N`. Taken out of the thread-local rather
    /// than borrowed, so a scan that re-enters another does not meet a held borrow.
    fn take<const N: usize>() -> Self {
        let mut scratch = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
        scratch.size_for::<N>();
        scratch
    }

    /// Hands the scratch back to this thread for the next scan.
    fn give_back(self) {
        SCRATCH.with(|s| *s.borrow_mut() = self);
    }

    /// Grows the buckets to one per feedback ordinal when the dense counter serves `N`.
    fn size_for<const N: usize>(&mut self) {
        if WordCorrectness::<N>::COUNT * BUCKET_BYTES <= DENSE_BOUND_BYTES {
            let need = usize::try_from(WordCorrectness::<N>::COUNT).unwrap_or(usize::MAX);
            if self.buckets.len() < need {
                self.buckets.resize(need, 0.0);
            }
        }
    }
}

impl Scratch {
    /// Adds `prob` to the dense bucket of `ordinal`.
    fn add_dense(&mut self, ordinal: u64, prob: f64) {
        debug_assert!(prob > 0.0, "a zero-probability word would be listed twice");
        let slot = usize::try_from(ordinal).unwrap_or(usize::MAX);
        if self.buckets[slot] == 0.0 {
            self.touched.push(ordinal);
        }
        self.buckets[slot] += prob;
    }

    /// Sums the dense buckets into an entropy, zeroing them.
    fn finish_dense(&mut self) -> f64 {
        let Self {
            buckets, touched, ..
        } = self;
        // ascending ordinal order keeps the floating-point sum independent of word order
        touched.sort_unstable();
        let entropy = entropy_from_masses(touched.iter().map(|&ordinal| {
            std::mem::take(&mut buckets[usize::try_from(ordinal).unwrap_or(usize::MAX)])
        }));
        touched.clear();
        entropy
    }

    /// Sums the sorting counter's pairs into an entropy. The sort is stable, so each run's mass
    /// accumulates in word order, as in the dense counter, and runs are summed in ascending
    /// ordinal.
    fn finish_sorted(&mut self) -> f64 {
        let pairs = &mut self.pairs;
        pairs.sort_by_key(|&(ordinal, _)| ordinal);
        let entropy = entropy_from_masses(
            pairs
                .chunk_by(|a, b| a.0 == b.0)
                .map(|run| run.iter().fold(0.0, |mass, &(_, prob)| mass + prob)),
        );
        pairs.clear();
        entropy
    }
}

/// Sums `p * log2(p)` over masses already in ascending ordinal order; both counters share it so
/// their floating-point results agree bit for bit.
fn entropy_from_masses(masses: impl Iterator<Item = f64>) -> f64 {
    let mut acc = 0.0_f64;
    for p in masses {
        acc = p.mul_add(p.log2(), acc);
    }
    // 0.0 - acc rather than -acc: a zero sum yields +0.0, not -0.0
    0.0 - acc
}

/// Whether 3^N buckets fit [`DENSE_BOUND_BYTES`], so the dense counter serves length `N`.
const fn is_dense<const N: usize>() -> bool {
    WordCorrectness::<N>::COUNT * BUCKET_BYTES <= DENSE_BOUND_BYTES
}

/// Expected information gain, in bits, of playing the guess of `plan` against the words of
/// `columns` weighted by `probs`, with one bucket per feedback ordinal for lengths whose 3^N
/// buckets fit [`DENSE_BOUND_BYTES`], else by sorting `(ordinal, probability)` pairs. Each block's
/// ordinals come from the kernel into a small buffer, and masses are added in word order.
fn guess_entropy_columns<const N: usize>(
    plan: &Plan<N>,
    columns: &Columns<N>,
    probs: &[f64],
    scratch: &mut Scratch,
) -> f64 {
    entropy_columns(plan, columns, probs, is_dense::<N>(), scratch)
}

/// [`guess_entropy_columns`] with the counter chosen by `dense`, so a test can run both. Ordinals
/// are `u32` while 3^N fits one, chosen at compile time.
fn entropy_columns<const N: usize>(
    plan: &Plan<N>,
    columns: &Columns<N>,
    probs: &[f64],
    dense: bool,
    scratch: &mut Scratch,
) -> f64 {
    if N <= MAX_U32_LENGTH {
        entropy_columns_as::<N, u32>(plan, columns, probs, dense, scratch)
    } else {
        entropy_columns_as::<N, u64>(plan, columns, probs, dense, scratch)
    }
}

/// [`entropy_columns`] with the kernel writing ordinals as `O`.
fn entropy_columns_as<const N: usize, O: Ordinal>(
    plan: &Plan<N>,
    columns: &Columns<N>,
    probs: &[f64],
    dense: bool,
    scratch: &mut Scratch,
) -> f64 {
    let mut ordinals = [O::default(); BLOCK];
    for k in 0..columns.blocks() {
        columns.ordinals(plan, k, &mut ordinals);
        let masses = &probs[k * BLOCK..][..columns.block_len(k)];
        if dense {
            for (&ordinal, &prob) in ordinals.iter().zip(masses) {
                scratch.add_dense(ordinal.into(), prob);
            }
        } else {
            scratch.pairs.extend(
                ordinals
                    .iter()
                    .map(|&o| o.into())
                    .zip(masses.iter().copied()),
            );
        }
    }
    if dense {
        scratch.finish_dense()
    } else {
        scratch.finish_sorted()
    }
}

/// [`guess_entropy_columns`] by the scalar `pattern_ordinal` over one code per word: the reference
/// the transposed scan must match bit for bit, and the "old" side of the benches.
#[cfg(any(test, feature = "bench"))]
fn guess_entropy<const N: usize>(
    guess: &Code<N>,
    words: &[Code<N>],
    probs: &[f64],
    scratch: &mut Scratch,
) -> f64 {
    entropy_scalar(guess, words, probs, is_dense::<N>(), scratch)
}

/// [`guess_entropy`] with the counter chosen by `dense`, so a test can run both.
#[cfg(any(test, feature = "bench"))]
fn entropy_scalar<const N: usize>(
    guess: &Code<N>,
    words: &[Code<N>],
    probs: &[f64],
    dense: bool,
    scratch: &mut Scratch,
) -> f64 {
    let mut counts: LetterCounts = [0; 256];
    for (word, &prob) in words.iter().zip(probs) {
        let ordinal = pattern_ordinal(word, guess, &mut counts);
        if dense {
            scratch.add_dense(ordinal, prob);
        } else {
            scratch.pairs.push((ordinal, prob));
        }
    }
    if dense {
        scratch.finish_dense()
    } else {
        scratch.finish_sorted()
    }
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
