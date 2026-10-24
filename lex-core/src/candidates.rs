use std::cell::RefCell;
use std::hint::cold_path;
use std::sync::Arc;

use lex_data::Word;

use crate::correctness::{WordCorrectness, pattern_ordinal};
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
/// guess pool, and the choice of the next guess under a [`Strategy`]. Holds no history and caches
/// nothing: a [`DecisionTree`] node owns one and memoizes its [`suggest`](Self::suggest).
///
/// [`DecisionTree`]: crate::tree::DecisionTree
#[derive(Debug, Clone)]
pub(crate) struct Candidates<const N: usize> {
    pool: Arc<Pool<N>>,
    /// Ascending, so frequency-descending.
    indices: Vec<u32>,
}

impl<const N: usize> Candidates<N> {
    pub(crate) fn new(pool: Arc<Pool<N>>) -> Self {
        let indices = (0..pool.len() as u32).collect();
        Self { pool, indices }
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
        }
    }

    pub(crate) fn pool(&self) -> Arc<Pool<N>> {
        Arc::clone(&self.pool)
    }

    pub(crate) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(crate) fn len(&self) -> usize {
        self.indices.len()
    }

    /// Calls `f(pool index, entropy, p_answer, in_candidates)` for every guess the strategy's pool
    /// allows, in frequency-descending order.
    fn for_each_guess(&self, strategy: &Strategy, mut f: impl FnMut(u32, f64, f64, bool)) {
        let probs = self.pool.prior(&self.indices, strategy.temperature);
        let words: Vec<Word<N>> = self.indices.iter().map(|&i| self.pool.word(i)).collect();
        match strategy.pool {
            GuessPool::Candidates => {
                for (pos, &i) in self.indices.iter().enumerate() {
                    f(
                        i,
                        guess_entropy(&words[pos], &words, &probs),
                        probs[pos],
                        true,
                    );
                }
            }
            GuessPool::All => {
                let mut next = 0;
                for i in 0..self.pool.len() as u32 {
                    let (p_answer, in_candidates) = match self.indices.get(next) {
                        Some(&c) if c == i => {
                            next += 1;
                            (probs[next - 1], true)
                        }
                        _ => (0.0, false),
                    };
                    f(
                        i,
                        guess_entropy(&self.pool.word(i), &words, &probs),
                        p_answer,
                        in_candidates,
                    );
                }
            }
        }
    }

    /// The `k` best guesses by score, descending. Equal scores prefer words still in the
    /// candidate set, then frequency order, so the first element agrees with
    /// [`suggest`](Self::suggest).
    pub(crate) fn suggest_top_k(&self, strategy: &Strategy, k: usize) -> Vec<Suggestion<N>> {
        let mut scored = Vec::new();
        self.for_each_guess(strategy, |i, entropy_bits, p_answer, in_candidates| {
            scored.push(Suggestion {
                word: self.pool.word(i),
                entropy_bits,
                p_answer,
                in_candidates,
            });
        });
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

    pub(crate) fn suggest(&self, strategy: &Strategy) -> Option<Word<N>> {
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
        self.for_each_guess(strategy, |i, entropy_bits, p_answer, in_candidates| {
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
        best.map(|(i, _, _)| self.pool.word(i))
    }
}

fn guess_entropy<const N: usize>(guess: &Word<N>, words: &[Word<N>], probs: &[f64]) -> f64 {
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
        if buckets.len() < WordCorrectness::<N>::COUNT {
            buckets.resize(WordCorrectness::<N>::COUNT, 0.0);
        }
        for (word, &prob) in words.iter().zip(probs) {
            debug_assert!(prob > 0.0, "a zero-probability word would be listed twice");
            let ordinal = pattern_ordinal(word, guess);
            if buckets[ordinal] == 0.0 {
                touched.push(ordinal as u32);
            }
            buckets[ordinal] += prob;
        }
        // ascending ordinal order keeps the floating-point sum independent of word order
        touched.sort_unstable();
        // 0.0 - acc rather than -acc: a zero sum yields +0.0, not -0.0
        let mut acc = 0.0_f64;
        for &ordinal in touched.iter() {
            let p = std::mem::take(&mut buckets[ordinal as usize]);
            acc += p * p.log2();
        }
        touched.clear();
        0.0 - acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_buffer_serves_every_word_length_on_one_thread() {
        let five = Word::<5>::try_from("crane").unwrap();
        let ten = Word::<10>::try_from("abcdefghij").unwrap();
        // N=5 sizes the shared buffer first; N=10 must grow it rather than index past the end
        assert_eq!(guess_entropy(&five, &[five], &[1.0]), 0.0);
        assert_eq!(guess_entropy(&ten, &[ten], &[1.0]), 0.0);
        assert_eq!(guess_entropy(&five, &[five], &[1.0]), 0.0);
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
        b.iter(|| black_box(guess_entropy(&guess, &words, &probs)));
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
        let candidates = Candidates::new(synthetic_pool::<N>(count));
        let strategy = Strategy::default();
        b.iter(|| black_box(candidates.suggest(&strategy)));
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
        let mut candidates = Candidates::new(synthetic_pool::<5>(3000));
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
        b.iter(|| black_box(candidates.suggest(&strategy)));
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
        let candidates = Candidates::new(synthetic_pool::<5>(1000));
        let target = candidates.pool.word(0);
        let guess = Guess::new(
            candidates.pool.word(1),
            WordCorrectness::correct(target, candidates.pool.word(1)),
        );
        b.iter(|| black_box(candidates.filtered(&guess)));
    }
}
