use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};
use std::hint::cold_path;
use std::sync::Arc;

use lex_data::Word;

use crate::correctness::{WordCorrectness, pattern_ordinal};
use crate::groups::Groups;
use crate::guesser::Guess;
use crate::strategy::{GuessPool, Pool, Strategy};

/// A scored guess: how much it is expected to teach, and whether it could itself be the answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion<const N: usize> {
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
    pub fn score(&self, win_weight: f64) -> f64 {
        self.entropy_bits + win_weight * self.p_answer
    }
}

/// The words still consistent with every guess played so far, as indices into the immutable
/// guess pool, the partition of the pool into [`Groups`] that split them identically, and the
/// choice of the next guess under a [`Strategy`]. Holds no history: a [`DecisionTree`] node owns
/// one and memoizes its [`suggest`](Self::suggest). Scoring coarsens the groups, which a child
/// then inherits.
///
/// [`DecisionTree`]: crate::tree::DecisionTree
#[derive(Debug, Clone)]
pub(crate) struct Candidates<const N: usize> {
    pool: Arc<Pool<N>>,
    /// Ascending, so frequency-descending.
    indices: Vec<u32>,
    groups: Groups,
}

/// Passes an already-mixed `u64` through, so the dedupe map does not hash a hash.
#[derive(Default)]
struct PassThrough(u64);

impl Hasher for PassThrough {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("only u64 keys are hashed");
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

fn hash_ordinals(ordinals: &[u64]) -> u64 {
    ordinals.iter().fold(0xcbf2_9ce4_8422_2325, |h, &x| {
        (h.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95)
    })
}

impl<const N: usize> Candidates<N> {
    /// Starts from exactly `answers`, as indices into the pool, so a Profile's Answer list and a
    /// Corpus run's every-word list take the same path.
    ///
    /// # Panics
    ///
    /// If an answer is not in the pool: `lex-data` guarantees the Answer list is a subset.
    pub(crate) fn new(pool: Arc<Pool<N>>, answers: &[Word<N>]) -> Self {
        let by_word: HashMap<Word<N>, u32> =
            (0..pool.len() as u32).map(|i| (pool.word(i), i)).collect();
        let mut indices: Vec<u32> = answers
            .iter()
            .map(|w| {
                *by_word
                    .get(w)
                    .unwrap_or_else(|| panic!("answer {w} is not in the guess pool"))
            })
            .collect();
        indices.sort_unstable();
        Self {
            pool,
            indices,
            groups: Groups::singletons(),
        }
    }

    /// Every pool word is a candidate.
    #[cfg(test)]
    pub(crate) fn whole_pool(pool: Arc<Pool<N>>) -> Self {
        let indices = (0..pool.len() as u32).collect();
        Self {
            pool,
            indices,
            groups: Groups::singletons(),
        }
    }

    /// The candidates that remain after `guess` is played.
    pub(crate) fn filtered(&self, guess: &Guess<N>) -> Self {
        let word = guess.word();
        let target_ordinal = guess.correctness().ordinal();
        let indices = self
            .indices
            .iter()
            .copied()
            .filter(|&i| pattern_ordinal(&self.pool.word(i), &word) == target_ordinal)
            .collect();
        Self {
            pool: Arc::clone(&self.pool),
            indices,
            groups: self.groups.clone(),
        }
    }

    pub(crate) fn pool(&self) -> Arc<Pool<N>> {
        Arc::clone(&self.pool)
    }

    pub(crate) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(crate) fn groups(&self) -> &Groups {
        &self.groups
    }

    pub(crate) fn len(&self) -> usize {
        self.indices.len()
    }

    /// Calls `f(pool index, entropy, p_answer, in_candidates)` for every guess the strategy's pool
    /// allows, in frequency-descending order, except that a pool word sharing a group with a lower
    /// one is left out (its score is the representative's). Returns the groups coarsened by any
    /// merge found while scoring, for the caller to adopt.
    ///
    /// Merge detection is exact: a group's feedback vector over the candidates is hashed, and on a
    /// hash match the earlier representative's vector is recomputed and compared in full. A hash
    /// collision between different vectors only forgoes a merge.
    fn for_each_guess(
        &self,
        strategy: &Strategy,
        mut f: impl FnMut(u32, f64, f64, bool),
    ) -> Option<Groups> {
        let probs = self.pool.prior(&self.indices, strategy.temperature);
        let words: Vec<Word<N>> = self.indices.iter().map(|&i| self.pool.word(i)).collect();
        let entropy_of = |guess: &Word<N>| {
            guess_entropy::<N>(words.iter().map(|w| pattern_ordinal(w, guess)), &probs)
        };
        match strategy.pool {
            // every group is a singleton in hard mode, so there is nothing to merge
            GuessPool::Candidates => {
                for (pos, &i) in self.indices.iter().enumerate() {
                    f(i, entropy_of(&words[pos]), probs[pos], true);
                }
                None
            }
            GuessPool::All => {
                let pool_len = self.pool.len();
                let vector = |guess: &Word<N>, out: &mut Vec<u64>| {
                    out.clear();
                    out.extend(words.iter().map(|w| pattern_ordinal(w, guess)));
                };
                let (mut ordinals, mut earlier) = (Vec::new(), Vec::new());
                let mut seen: HashMap<u64, usize, BuildHasherDefault<PassThrough>> =
                    HashMap::default();
                let mut merges = Vec::new();
                let mut next = 0;
                for (group, (rep, extra)) in self.groups.iter(pool_len).enumerate() {
                    let guess = self.pool.word(rep);
                    if self.indices.get(next) == Some(&rep) {
                        // a candidate gives all-correct against itself and no other word does,
                        // so it never shares a group
                        debug_assert!(extra.is_empty());
                        f(rep, entropy_of(&guess), probs[next], true);
                        next += 1;
                        continue;
                    }
                    vector(&guess, &mut ordinals);
                    match seen.entry(hash_ordinals(&ordinals)) {
                        Entry::Occupied(e) => {
                            let kept = *e.get();
                            vector(&self.pool.word(self.groups.group(kept).0), &mut earlier);
                            if earlier == ordinals {
                                merges.push((kept, group));
                                continue;
                            }
                        }
                        Entry::Vacant(e) => {
                            e.insert(group);
                        }
                    }
                    f(
                        rep,
                        guess_entropy::<N>(ordinals.iter().copied(), &probs),
                        0.0,
                        false,
                    );
                }
                (!merges.is_empty()).then(|| self.groups.coarsened(pool_len, &mut merges))
            }
        }
    }

    /// The `k` best guesses by score, descending. Equal scores prefer words still in the
    /// candidate set, then frequency order, so the first element agrees with
    /// [`suggest`](Self::suggest). Every pool word is listed: a word that shares a group with a
    /// lower one carries that representative's score.
    pub(crate) fn suggest_top_k(&mut self, strategy: &Strategy, k: usize) -> Vec<Suggestion<N>> {
        let mut scored = Vec::new();
        let merged = self.for_each_guess(strategy, |i, entropy_bits, p_answer, in_candidates| {
            scored.push(Suggestion {
                word: self.pool.word(i),
                entropy_bits,
                p_answer,
                in_candidates,
            });
        });
        if let Some(groups) = merged {
            self.groups = groups;
        }
        if strategy.pool == GuessPool::All {
            // scored holds one entry per group, in group order
            let mut expanded: Vec<(u32, Suggestion<N>)> = Vec::with_capacity(self.pool.len());
            for (s, (rep, extra)) in scored.iter().zip(self.groups.iter(self.pool.len())) {
                expanded.push((rep, *s));
                expanded.extend(extra.iter().map(|&i| {
                    let member = Suggestion {
                        word: self.pool.word(i),
                        p_answer: 0.0,
                        in_candidates: false,
                        ..*s
                    };
                    (i, member)
                }));
            }
            expanded.sort_unstable_by_key(|&(i, _)| i);
            scored = expanded.into_iter().map(|(_, s)| s).collect();
        }
        // stable sort: equal keys keep frequency-descending iteration order
        scored.sort_by(|a, b| {
            let (sa, sb) = (a.score(strategy.win_weight), b.score(strategy.win_weight));
            sb.partial_cmp(&sa)
                .expect("scores are finite")
                .then(b.in_candidates.cmp(&a.in_candidates))
        });
        scored.truncate(k);
        scored
    }

    pub(crate) fn suggest(&mut self, strategy: &Strategy) -> Option<Word<N>> {
        match self.indices.as_slice() {
            [] => {
                cold_path();
                return None;
            }
            // every guess scores zero entropy against a single candidate, so it is the answer
            &[only] => return Some(self.pool.word(only)),
            _ => {}
        }

        // (index, score, in_candidates)
        let mut best: Option<(u32, f64, bool)> = None;
        let merged = self.for_each_guess(strategy, |i, entropy_bits, p_answer, in_candidates| {
            let score = entropy_bits + strategy.win_weight * p_answer;
            log::trace!("{}: {}", self.pool.word(i), score);
            match best {
                Some((best_i, best_score, best_in)) => {
                    if score > best_score || (score == best_score && in_candidates && !best_in) {
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
        });
        if let Some(groups) = merged {
            self.groups = groups;
        }
        best.map(|(i, _, _)| self.pool.word(i))
    }
}
/// Largest bucket array the dense counter may allocate per thread. Working memory like this sits
/// outside the 1 GB corpus cap; the bound makes the dense/sorted cut-over a memory rule rather
/// than a list of word lengths.
const DENSE_BOUND_BYTES: u64 = 16 * 1024 * 1024;

/// Entropy of the feedback distribution, given each candidate's feedback ordinal against one
/// guess and its probability.
fn guess_entropy<const N: usize>(ordinals: impl Iterator<Item = u64>, probs: &[f64]) -> f64 {
    if WordCorrectness::<N>::COUNT * size_of::<f64>() as u64 <= DENSE_BOUND_BYTES {
        entropy_dense::<N>(ordinals, probs)
    } else {
        entropy_sorted(ordinals, probs)
    }
}

/// Sums `p * log2(p)` over masses already in ascending ordinal order; both counters share it so
/// their floating-point results agree bit for bit.
fn entropy_from_masses(masses: impl Iterator<Item = f64>) -> f64 {
    let mut acc = 0.0_f64;
    for p in masses {
        acc += p * p.log2();
    }
    // 0.0 - acc rather than -acc: a zero sum yields +0.0, not -0.0
    0.0 - acc
}

fn entropy_dense<const N: usize>(ordinals: impl Iterator<Item = u64>, probs: &[f64]) -> f64 {
    /// Probability mass per feedback ordinal. Every bucket is zero between calls; `touched` lists
    /// the ones a call dirtied, so neither the reset nor the sum walks all 3^N buckets.
    struct Patterns {
        buckets: Vec<f64>,
        touched: Vec<u32>,
    }
    thread_local! {
        static PATTERNS: RefCell<Patterns> = const {
            RefCell::new(Patterns { buckets: Vec::new(), touched: Vec::new() })
        };
    }
    PATTERNS.with(|patterns| {
        let Patterns { buckets, touched } = &mut *patterns.borrow_mut();
        // one static serves every monomorphization, so it grows to the largest N seen
        if (buckets.len() as u64) < WordCorrectness::<N>::COUNT {
            buckets.resize(WordCorrectness::<N>::COUNT as usize, 0.0);
        }
        for (ordinal, &prob) in ordinals.zip(probs) {
            debug_assert!(prob > 0.0, "a zero-probability word would be listed twice");
            let slot = ordinal as usize;
            if buckets[slot] == 0.0 {
                touched.push(ordinal as u32);
            }
            buckets[slot] += prob;
        }
        // ascending ordinal order keeps the floating-point sum independent of word order
        touched.sort_unstable();
        let entropy = entropy_from_masses(
            touched
                .iter()
                .map(|&ordinal| std::mem::take(&mut buckets[ordinal as usize])),
        );
        touched.clear();
        entropy
    })
}

/// Counts by sorting `(ordinal, probability)` pairs, for lengths whose dense array would exceed
/// [`DENSE_BOUND_BYTES`]. The sort is stable, so each run's mass accumulates in word order, as in
/// the dense counter, and runs are summed in ascending ordinal.
fn entropy_sorted(ordinals: impl Iterator<Item = u64>, probs: &[f64]) -> f64 {
    thread_local! {
        static SCRATCH: RefCell<Vec<(u64, f64)>> = const { RefCell::new(Vec::new()) };
    }
    SCRATCH.with(|scratch| {
        let scratch = &mut *scratch.borrow_mut();
        scratch.clear();
        scratch.extend(ordinals.zip(probs.iter().copied()));
        scratch.sort_by_key(|&(ordinal, _)| ordinal);
        let entropy = entropy_from_masses(
            scratch
                .chunk_by(|a, b| a.0 == b.0)
                .map(|run| run.iter().fold(0.0, |mass, &(_, prob)| mass + prob)),
        );
        scratch.clear();
        entropy
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entropy_against<const N: usize>(guess: &Word<N>, words: &[Word<N>], probs: &[f64]) -> f64 {
        guess_entropy::<N>(words.iter().map(|w| pattern_ordinal(w, guess)), probs)
    }

    #[test]
    fn entropy_buffer_serves_every_word_length_on_one_thread() {
        let five = Word::<5>::try_from("crane").unwrap();
        let ten = Word::<10>::try_from("abcdefghij").unwrap();
        // N=5 sizes the shared buffer first; N=10 must grow it rather than index past the end
        assert_eq!(entropy_against(&five, &[five], &[1.0]), 0.0);
        assert_eq!(entropy_against(&ten, &[ten], &[1.0]), 0.0);
        assert_eq!(entropy_against(&five, &[five], &[1.0]), 0.0);
    }

    use proptest::prelude::*;

    /// Words over a small alphabet so patterns collide and runs have several members.
    fn counters_agree<const N: usize>(
        guess: [u8; N],
        pairs: Vec<([u8; N], f64)>,
    ) -> Result<(), TestCaseError> {
        let word = |bytes: [u8; N]| Word::<N>::from(bytes.map(|b| char::from(b'a' + b)));
        let guess = word(guess);
        let words: Vec<Word<N>> = pairs.iter().map(|&(w, _)| word(w)).collect();
        let probs: Vec<f64> = pairs.iter().map(|&(_, p)| p).collect();
        let ordinals = || words.iter().map(|w| pattern_ordinal(w, &guess));
        prop_assert_eq!(
            entropy_dense::<N>(ordinals(), &probs).to_bits(),
            entropy_sorted(ordinals(), &probs).to_bits()
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn dense_and_sorted_agree_n3(
            guess in prop::array::uniform3(0u8..3),
            pairs in prop::collection::vec((prop::array::uniform3(0u8..3), 1e-6f64..1.0), 1..60),
        ) {
            counters_agree::<3>(guess, pairs)?;
        }

        #[test]
        fn dense_and_sorted_agree_n5(
            guess in prop::array::uniform5(0u8..4),
            pairs in prop::collection::vec((prop::array::uniform5(0u8..4), 1e-6f64..1.0), 1..80),
        ) {
            counters_agree::<5>(guess, pairs)?;
        }

        #[test]
        fn dense_and_sorted_agree_n13(
            guess in prop::array::uniform13(0u8..4),
            pairs in prop::collection::vec((prop::array::uniform13(0u8..4), 1e-6f64..1.0), 1..80),
        ) {
            counters_agree::<13>(guess, pairs)?;
        }
    }

    #[test]
    fn dense_bound_is_a_memory_rule() {
        const { assert!(WordCorrectness::<13>::COUNT * 8 <= DENSE_BOUND_BYTES) };
        const { assert!(WordCorrectness::<14>::COUNT * 8 > DENSE_BOUND_BYTES) };
    }
}

#[cfg(test)]
mod dedupe_tests {
    use lex_data::WordSet;

    use super::*;
    use crate::correctness::WordCorrectness;

    /// A reproducible pool over a six-letter alphabet, so many words split a small candidate set
    /// identically.
    fn small_alphabet_pool(count: usize) -> Arc<Pool<5>> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut seen = std::collections::HashSet::new();
        let mut pairs = Vec::new();
        while pairs.len() < count {
            let chars: [char; 5] = std::array::from_fn(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                char::from(b'a' + ((state >> 33) % 6) as u8)
            });
            if seen.insert(chars) {
                pairs.push((Word::<5>::from(chars), (count - pairs.len()) as u64 * 10));
            }
        }
        Pool::new(&WordSet::from_pairs(pairs))
    }

    /// Every pool word scored from scratch, one entropy per word, ordered as `suggest_top_k`
    /// promises.
    fn reference(c: &Candidates<5>, strategy: &Strategy) -> Vec<Suggestion<5>> {
        let probs = c.pool.prior(&c.indices, strategy.temperature);
        let words: Vec<Word<5>> = c.indices.iter().map(|&i| c.pool.word(i)).collect();
        let mut all: Vec<Suggestion<5>> = (0..c.pool.len() as u32)
            .map(|i| {
                let pos = c.indices.binary_search(&i).ok();
                Suggestion {
                    word: c.pool.word(i),
                    entropy_bits: entropy_dense::<5>(
                        words.iter().map(|w| pattern_ordinal(w, &c.pool.word(i))),
                        &probs,
                    ),
                    p_answer: pos.map_or(0.0, |p| probs[p]),
                    in_candidates: pos.is_some(),
                }
            })
            .collect();
        all.sort_by(|a, b| {
            let (sa, sb) = (a.score(strategy.win_weight), b.score(strategy.win_weight));
            sb.partial_cmp(&sa)
                .unwrap()
                .then(b.in_candidates.cmp(&a.in_candidates))
        });
        all
    }

    #[test]
    fn suggest_and_top_k_are_unchanged_by_dedupe() {
        let pool = small_alphabet_pool(400);
        for win_weight in [0.0, 1.5] {
            let strategy = Strategy {
                pool: GuessPool::All,
                win_weight,
                ..Strategy::default()
            };
            let mut merged_somewhere = false;
            for target in [3, 90, 250] {
                let target = pool.word(target);
                let mut c = Candidates::whole_pool(Arc::clone(&pool));
                while c.len() > 1 {
                    let expected = reference(&c, &strategy);
                    let mut for_top_k = c.clone();
                    assert_eq!(for_top_k.suggest_top_k(&strategy, usize::MAX), expected);
                    let mut for_suggest = c.clone();
                    let guess = for_suggest.suggest(&strategy).unwrap();
                    assert_eq!(guess, expected[0].word);
                    merged_somewhere |= for_suggest.groups.len(pool.len()) < pool.len();
                    // the child inherits the coarsened groups
                    let feedback = WordCorrectness::correct(target, guess);
                    c = for_suggest.filtered(&Guess::new(guess, feedback));
                }
            }
            assert!(merged_somewhere, "no merge: the test would prove nothing");
        }
    }

    #[test]
    fn hard_mode_groups_stay_singletons() {
        let pool = small_alphabet_pool(100);
        let mut c = Candidates::whole_pool(Arc::clone(&pool));
        c.suggest(&Strategy::hard_mode());
        assert_eq!(c.groups.len(pool.len()), pool.len());
    }
}

#[cfg(test)]
mod benches {
    extern crate test;
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
        let words: Vec<Word<5>> = pairs.iter().map(|&(w, _)| w).collect();
        let probs = vec![0.1; words.len()];
        b.iter(|| {
            black_box(guess_entropy::<5>(
                words.iter().map(|w| pattern_ordinal(w, &guess)),
                &probs,
            ))
        });
    }

    /// A reproducible pool of `count` distinct words over a..z, frequency-descending, so every
    /// bench scores against letter overlap like a real corpus rather than disjoint words.
    fn synthetic_pool<const N: usize>(count: usize) -> Arc<Pool<N>> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut seen = std::collections::HashSet::new();
        let mut pairs = Vec::with_capacity(count);
        while pairs.len() < count {
            let chars: [char; N] = std::array::from_fn(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                char::from(b'a' + ((state >> 33) % 26) as u8)
            });
            if seen.insert(chars) {
                pairs.push((Word::<N>::from(chars), (count - pairs.len()) as u64 * 10));
            }
        }
        Pool::new(&lex_data::WordSet::from_pairs(pairs))
    }

    fn bench_suggest<const N: usize>(b: &mut Bencher, count: usize) {
        let candidates = Candidates::whole_pool(synthetic_pool::<N>(count));
        let strategy = Strategy::default();
        b.iter(|| black_box(candidates.clone().suggest(&strategy)));
    }

    #[bench]
    fn root_suggest_all_1000_n5(b: &mut Bencher) {
        bench_suggest::<5>(b, 1000);
    }

    #[bench]
    fn root_suggest_all_300_n10(b: &mut Bencher) {
        bench_suggest::<10>(b, 300);
    }

    /// Narrows a 3000-word pool with its most frequent words as guesses, as a game would, until at
    /// most `limit` candidates remain, then times a full-pool `suggest`. Most nodes of a full run
    /// are deep ones like this: few candidates scored against a large pool.
    fn bench_deep_suggest(b: &mut Bencher, limit: usize) {
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
        b.iter(|| black_box(candidates.clone().suggest(&strategy)));
    }

    #[bench]
    fn deep_suggest_all_60_of_3000_n5(b: &mut Bencher) {
        bench_deep_suggest(b, 60);
    }

    #[bench]
    fn deep_suggest_all_8_of_3000_n5(b: &mut Bencher) {
        bench_deep_suggest(b, 8);
    }

    #[bench]
    fn filtered_1000_n5(b: &mut Bencher) {
        let candidates = Candidates::whole_pool(synthetic_pool::<5>(1000));
        let target = candidates.pool.word(0);
        let guess = Guess::new(
            candidates.pool.word(1),
            WordCorrectness::correct(target, candidates.pool.word(1)),
        );
        b.iter(|| black_box(candidates.filtered(&guess)));
    }
}
