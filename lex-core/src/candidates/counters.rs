//! The entropy scan's counters: dense buckets or sort-and-run, over a thread-local scratch.

use std::cell::RefCell;

use crate::correctness::{Code, LetterCounts, WordCorrectness, pattern_ordinal};
use crate::transposed::{BLOCK, Columns, MAX_U32_LENGTH, Ordinal, Plan};

/// Largest bucket array the dense counter may allocate per thread. Working memory like this sits
/// outside the 1 GB corpus cap; the bound makes the dense/sorted cut-over a memory rule rather
/// than a list of word lengths.
pub(super) const DENSE_BOUND_BYTES: u64 = 16 * 1024 * 1024;

/// Bytes one dense bucket takes.
pub(super) const BUCKET_BYTES: u64 = 8;
const _: () = assert!(size_of::<f64>() == 8, "a dense bucket is one f64");

/// The per-thread working memory of an entropy scan, taken once per scan and reused for every
/// guess in it.
#[derive(Default)]
pub(super) struct Scratch {
    /// Probability mass per feedback ordinal. Every bucket is zero between guesses; `touched`
    /// lists the ones a guess dirtied, so neither the reset nor the sum walks all 3^N buckets.
    pub(super) buckets: Vec<f64>,
    /// Ordinals of the dirtied buckets.
    pub(super) touched: Vec<u64>,
    /// `(ordinal, probability)` pairs for the sorting counter.
    pub(super) pairs: Vec<(u64, f64)>,
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
    pub(super) fn take<const N: usize>() -> Self {
        let mut scratch = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
        scratch.size_for::<N>();
        scratch
    }

    /// Hands the scratch back to this thread for the next scan.
    pub(super) fn give_back(self) {
        SCRATCH.with(|s| *s.borrow_mut() = self);
    }

    /// Grows the buckets to one per feedback ordinal when the dense counter serves `N`.
    pub(super) fn size_for<const N: usize>(&mut self) {
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
    pub(super) fn add_dense(&mut self, ordinal: u64, prob: f64) {
        debug_assert!(prob > 0.0, "a zero-probability word would be listed twice");
        let slot = usize::try_from(ordinal).unwrap_or(usize::MAX);
        if self.buckets[slot] == 0.0 {
            self.touched.push(ordinal);
        }
        self.buckets[slot] += prob;
    }

    /// Sums the dense buckets into an entropy, zeroing them.
    pub(super) fn finish_dense(&mut self) -> f64 {
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
    pub(super) fn finish_sorted(&mut self) -> f64 {
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
pub(super) fn entropy_from_masses(masses: impl Iterator<Item = f64>) -> f64 {
    let mut acc = 0.0_f64;
    for p in masses {
        acc = p.mul_add(p.log2(), acc);
    }
    // 0.0 - acc rather than -acc: a zero sum yields +0.0, not -0.0
    0.0 - acc
}

/// Whether 3^N buckets fit [`DENSE_BOUND_BYTES`], so the dense counter serves length `N`.
pub(super) const fn is_dense<const N: usize>() -> bool {
    WordCorrectness::<N>::COUNT * BUCKET_BYTES <= DENSE_BOUND_BYTES
}

/// Expected information gain, in bits, of playing the guess of `plan` against the words of
/// `columns` weighted by `probs`, with one bucket per feedback ordinal for lengths whose 3^N
/// buckets fit [`DENSE_BOUND_BYTES`], else by sorting `(ordinal, probability)` pairs. Each block's
/// ordinals come from the kernel into a small buffer, and masses are added in word order.
pub(super) fn guess_entropy_columns<const N: usize>(
    plan: &Plan<N>,
    columns: &Columns<N>,
    probs: &[f64],
    scratch: &mut Scratch,
) -> f64 {
    entropy_columns(plan, columns, probs, is_dense::<N>(), scratch)
}

/// [`guess_entropy_columns`] with the counter chosen by `dense`, so a test can run both. Ordinals
/// are `u32` while 3^N fits one, chosen at compile time.
pub(super) fn entropy_columns<const N: usize>(
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
pub(super) fn entropy_columns_as<const N: usize, O: Ordinal>(
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
pub(super) fn guess_entropy<const N: usize>(
    guess: &Code<N>,
    words: &[Code<N>],
    probs: &[f64],
    scratch: &mut Scratch,
) -> f64 {
    entropy_scalar(guess, words, probs, is_dense::<N>(), scratch)
}

/// [`guess_entropy`] with the counter chosen by `dense`, so a test can run both.
#[cfg(any(test, feature = "bench"))]
pub(super) fn entropy_scalar<const N: usize>(
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
