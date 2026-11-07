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
use crate::transposed::{BLOCK, Columns, Plan};

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
/// nothing: a [`DecisionTree`] node owns one and memoizes its [`suggest`](Self::suggest).
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
    /// [`suggest`](Self::suggest). Only the `k` kept are sorted.
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
        // the pool index closes the order, so the unstable selection is deterministic
        let better = |a: &Scored, b: &Scored| {
            cmp_scores(b.score(strategy.win_weight), a.score(strategy.win_weight))
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
        scored
    }

    /// The guess with the best score among those `allowed` permits, preferring a Candidate on ties; `None` when the set is empty.
    pub(crate) fn suggest(&self, strategy: &Strategy, allowed: &Allowed) -> Option<Word<N>> {
        self.suggest_index(strategy, allowed)
            .map(|index| self.pool.word(index))
    }

    /// [`suggest`](Self::suggest) as a pool index.
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

/// [`guess_entropy_columns`] with the counter chosen by `dense`, so a test can run both.
fn entropy_columns<const N: usize>(
    plan: &Plan<N>,
    columns: &Columns<N>,
    probs: &[f64],
    dense: bool,
    scratch: &mut Scratch,
) -> f64 {
    let mut ordinals = [0_u64; BLOCK];
    for k in 0..columns.blocks() {
        columns.ordinals(plan, k, &mut ordinals);
        let masses = &probs[k * BLOCK..][..columns.block_len(k)];
        if dense {
            for (&ordinal, &prob) in ordinals.iter().zip(masses) {
                scratch.add_dense(ordinal, prob);
            }
        } else {
            scratch
                .pairs
                .extend(ordinals.iter().copied().zip(masses.iter().copied()));
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
mod tests {
    use super::*;

    #[test]
    fn entropy_buffer_serves_every_word_length_on_one_thread() {
        let five: Code<5> = [0, 1, 2, 3, 4];
        let ten: Code<10> = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        // N=5 sizes the shared buffer first; N=10 must grow it rather than index past the end
        let mut scratch = Scratch::take::<5>();
        assert_eq!(
            guess_entropy(&five, &[five], &[1.0], &mut scratch)
                .abs()
                .to_bits(),
            0
        );
        scratch.give_back();
        let mut scratch = Scratch::take::<10>();
        assert_eq!(
            guess_entropy(&ten, &[ten], &[1.0], &mut scratch)
                .abs()
                .to_bits(),
            0
        );
        scratch.give_back();
        let mut scratch = Scratch::take::<5>();
        assert_eq!(
            guess_entropy(&five, &[five], &[1.0], &mut scratch)
                .abs()
                .to_bits(),
            0
        );
        scratch.give_back();
    }

    #[test]
    fn filtered_by_a_guess_with_letters_outside_the_alphabet() {
        let pairs = [("crane", 50), ("stare", 40), ("light", 30), ("shore", 20)]
            .map(|(s, f)| (Word::<5>::try_from(s).unwrap(), f));
        let candidates =
            Candidates::whole_pool(Pool::new(&lex_data::WordSet::from_pairs(pairs.to_vec())));
        // z, q and x are in no pool word
        let guess = Word::<5>::try_from("zqxre").unwrap();
        let target = candidates.pool.word(0);
        let kept = candidates.filtered(&Guess::new(guess, WordCorrectness::correct(target, guess)));
        let expect: Vec<u32> = (0..4)
            .filter(|&i| {
                WordCorrectness::correct(candidates.pool.word(i), guess)
                    == WordCorrectness::correct(target, guess)
            })
            .collect();
        assert_eq!(kept.indices(), expect);
        assert!(kept.indices().contains(&0));
    }

    /// Forty distinct words over a small alphabet, frequency-descending, so scores collide and
    /// the tie-break order matters.
    fn top_k_pool() -> Arc<Pool<5>> {
        let letters = ['a', 'b', 'c', 'd'];
        let mut seen = std::collections::HashSet::new();
        let pairs: Vec<(Word<5>, u64)> = (0..400_u64)
            .map(|i| -> [char; 5] {
                std::array::from_fn(|p| {
                    letters[usize::try_from(i * 7 / 3 + i * i).unwrap_or(0) >> (2 * p) & 3]
                })
            })
            .filter(|chars| seen.insert(*chars))
            .take(40)
            .zip(0_u64..)
            .map(|(chars, i)| (Word::from(chars), 400 - i))
            .collect();
        assert_eq!(
            pairs.len(),
            40,
            "the generator yields enough distinct words"
        );
        Pool::new(&lex_data::WordSet::from_pairs(pairs))
    }

    #[test]
    fn top_k_matches_the_full_sort_and_leads_with_suggest() {
        let pool = top_k_pool();
        let all: Vec<u32> = pool.indices().collect();
        let strategy = crate::strategy::Strategy::default();
        let sets = [
            all.iter().copied().step_by(2).collect::<Vec<_>>(),
            all.iter().copied().skip(5).take(9).collect::<Vec<_>>(),
            all.clone(),
        ];
        for set in sets {
            let candidates = Candidates::from_indices(Arc::clone(&pool), set.clone());
            let list = Allowed::List(
                all.iter()
                    .copied()
                    .step_by(3)
                    .chain(set.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            );
            for allowed in [Allowed::Whole, Allowed::Candidates, list] {
                // the reference scores every guess, then sorts all of them with a stable sort
                let mut full = Vec::new();
                candidates.for_each_guess(&allowed, strategy.temperature, |index, e, p, c| {
                    full.push(Scored {
                        index,
                        entropy_bits: e,
                        p_answer: p,
                        in_candidates: c,
                    });
                });
                full.sort_by(|a, b| {
                    cmp_scores(b.score(strategy.win_weight), a.score(strategy.win_weight))
                        .then(b.in_candidates.cmp(&a.in_candidates))
                });
                for k in [0, 1, 2, 7, full.len(), full.len() + 3] {
                    let top = candidates.suggest_top_k(&strategy, &allowed, k);
                    assert_eq!(top, full[..k.min(full.len())], "k {k}");
                }
                assert_eq!(
                    candidates.suggest_top_k(&strategy, &allowed, 1)[0].index,
                    candidates.suggest_index(&strategy, &allowed).unwrap()
                );
            }
        }
    }

    #[test]
    fn the_scan_scores_every_guess_as_the_scalar_counter_does() {
        let pool = top_k_pool();
        let all: Vec<u32> = pool.indices().collect();
        let strategy = crate::strategy::Strategy::default();
        // a whole pool, a strided subset and a single word; blocks past one are the proptests'
        for set in [
            all.clone(),
            all.iter().copied().step_by(3).collect::<Vec<_>>(),
            all[..1].to_vec(),
        ] {
            let candidates = Candidates::from_indices(Arc::clone(&pool), set.clone());
            let probs = pool.prior(&set, strategy.temperature);
            let words: Vec<Code<5>> = set.iter().map(|&i| *pool.code(i)).collect();
            let mut scratch = Scratch::take::<5>();
            let mut seen = 0;
            candidates.for_each_guess(&Allowed::Whole, strategy.temperature, |i, entropy, _, _| {
                let expect = guess_entropy(pool.code(i), &words, &probs, &mut scratch);
                assert_eq!(entropy.to_bits(), expect.to_bits(), "guess {i}");
                seen += 1;
            });
            scratch.give_back();
            assert_eq!(seen, pool.len());
        }
    }

    use proptest::prelude::*;

    /// Words over a small alphabet so patterns collide and runs have several members.
    fn counters_agree<const N: usize>(
        guess: [u8; N],
        pairs: &[([u8; N], f64)],
    ) -> Result<(), TestCaseError> {
        let words: Vec<Code<N>> = pairs.iter().map(|&(w, _)| w).collect();
        let probs: Vec<f64> = pairs.iter().map(|&(_, p)| p).collect();
        let columns = Columns::gather(words.iter(), 255);
        let plan = Plan::new(&guess);
        let mut scratch = Scratch::take::<N>();
        // the sorting counter is the one every length can run; the dense one only where it fits
        let mut bits = vec![
            entropy_scalar(&guess, &words, &probs, false, &mut scratch).to_bits(),
            entropy_columns(&plan, &columns, &probs, false, &mut scratch).to_bits(),
        ];
        if is_dense::<N>() {
            bits.push(entropy_scalar(&guess, &words, &probs, true, &mut scratch).to_bits());
            bits.push(entropy_columns(&plan, &columns, &probs, true, &mut scratch).to_bits());
        }
        scratch.give_back();
        prop_assert!(bits.iter().all(|&b| b == bits[0]), "{bits:?}");
        Ok(())
    }

    proptest! {
        #[test]
        fn counters_agree_n17(
            guess in prop::array::uniform17(0u8..5),
            pairs in prop::collection::vec((prop::array::uniform17(0u8..5), 1e-6f64..1.0), 1..150),
        ) {
            counters_agree::<17>(guess, &pairs)?;
        }

        #[test]
        fn counters_agree_n25(
            guess in prop::array::uniform25(0u8..3),
            pairs in prop::collection::vec((prop::array::uniform25(0u8..3), 1e-6f64..1.0), 1..150),
        ) {
            counters_agree::<25>(guess, &pairs)?;
        }

        #[test]
        fn dense_and_sorted_agree_n3(
            guess in prop::array::uniform3(0u8..3),
            pairs in prop::collection::vec((prop::array::uniform3(0u8..3), 1e-6f64..1.0), 1..60),
        ) {
            counters_agree::<3>(guess, &pairs)?;
        }

        #[test]
        fn dense_and_sorted_agree_n5(
            guess in prop::array::uniform5(0u8..4),
            pairs in prop::collection::vec((prop::array::uniform5(0u8..4), 1e-6f64..1.0), 1..80),
        ) {
            counters_agree::<5>(guess, &pairs)?;
        }

        #[test]
        fn dense_and_sorted_agree_n13(
            guess in prop::array::uniform13(0u8..4),
            pairs in prop::collection::vec((prop::array::uniform13(0u8..4), 1e-6f64..1.0), 1..80),
        ) {
            counters_agree::<13>(guess, &pairs)?;
        }
    }

    #[test]
    fn dense_bound_is_a_memory_rule() {
        const { assert!(WordCorrectness::<13>::COUNT * 8 <= DENSE_BOUND_BYTES) };
        const { assert!(WordCorrectness::<14>::COUNT * 8 > DENSE_BOUND_BYTES) };
    }
}

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
mod benches {
    use divan::Bencher;
    use std::hint::black_box;

    use super::{
        Allowed, Candidates, Code, Guess, Pool, Scratch, Strategy, Word, WordCorrectness,
        guess_entropy, synthetic_pool,
    };

    #[divan::bench]
    fn guess_entropy_10_words(b: Bencher) {
        let pairs: Vec<(Word<5>, u64)> = (0..10_u64)
            .map(|i| (Word::from([crate::guesser::letter(i); 5]), 10 - i))
            .collect();
        let pool = Pool::new(&lex_data::WordSet::from_pairs(pairs));
        let guess = *pool.code(0);
        let words: Vec<Code<5>> = pool.indices().map(|i| *pool.code(i)).collect();
        let probs = vec![0.1; words.len()];
        let mut scratch = Scratch::take::<5>();
        b.bench_local(|| black_box(guess_entropy(&guess, &words, &probs, &mut scratch)));
    }

    fn bench_suggest<const N: usize>(b: Bencher, count: usize) {
        let candidates = Candidates::whole_pool(synthetic_pool::<N>(count));
        let strategy = Strategy::default();
        b.bench(|| black_box(candidates.suggest(&strategy, &Allowed::Whole)));
    }

    #[divan::bench]
    fn root_suggest_all_1000_n5(b: Bencher) {
        bench_suggest::<5>(b, 1000);
    }

    #[divan::bench]
    fn root_suggest_all_300_n10(b: Bencher) {
        bench_suggest::<10>(b, 300);
    }

    /// Narrows a 3000-word pool with its most frequent words as guesses, as a game would, until at
    /// most `limit` candidates remain, then times a full-pool `suggest`. Most nodes of a full run
    /// are deep ones like this: few candidates scored against a large pool.
    fn bench_deep_suggest(b: Bencher, limit: usize) {
        let mut candidates = Candidates::whole_pool(synthetic_pool::<5>(3000));
        let target = candidates.pool.word(1);
        for i in 2..400 {
            if candidates.len() <= limit {
                break;
            }
            let word = candidates.pool.word(i);
            let feedback = WordCorrectness::correct(target, word);
            candidates = candidates.filtered(&Guess::new(word, feedback));
        }
        assert!(
            (2..=limit).contains(&candidates.len()),
            "{}",
            candidates.len()
        );
        let strategy = Strategy::default();
        b.bench(|| black_box(candidates.suggest(&strategy, &Allowed::Whole)));
    }

    #[divan::bench]
    fn deep_suggest_all_60_of_3000_n5(b: Bencher) {
        bench_deep_suggest(b, 60);
    }

    #[divan::bench]
    fn deep_suggest_all_8_of_3000_n5(b: Bencher) {
        bench_deep_suggest(b, 8);
    }

    #[divan::bench]
    fn filtered_1000_n5(b: Bencher) {
        let candidates = Candidates::whole_pool(synthetic_pool::<5>(1000));
        let target = candidates.pool.word(0);
        let guess = Guess::new(
            candidates.pool.word(1),
            WordCorrectness::correct(target, candidates.pool.word(1)),
        );
        b.bench(|| black_box(candidates.filtered(&guess)));
    }
}
