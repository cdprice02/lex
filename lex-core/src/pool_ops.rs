//! The word-length-free face of a Guess pool.
//!
//! The Endgame, the rollout and the Base policy only pass `N` through to a handful of pool
//! operations, so they are written against this trait and compiled once. Each operation is coarse
//! (a whole partition, scan or table), so the dynamic call is not a cost. The generic work stays
//! in [`Pool`] and [`Candidates`], reached through the shared pool.

use std::fmt::Debug;
use std::ops::Range;
use std::sync::Arc;

use crate::allowed::Allowed;
use crate::candidates::{Candidates, Scored};
use crate::correctness::WordCorrectness;
use crate::endgame::PatternTable;
use crate::strategy::{GuessPool, Pool, Strategy};

/// What the search needs of a Guess pool of any word length. Set arguments are ascending pool
/// indices.
pub trait PoolOps: Debug {
    /// Number of words in the pool.
    fn len(&self) -> usize;

    /// Every pool index, ascending.
    fn indices(&self) -> Range<u32>;

    /// The feedback ordinal of an all-green guess.
    fn win_ordinal(&self) -> u64;

    /// Unnormalized answer prior weight of every pool word, see [`Pool::weights`].
    fn weights(&self, temperature: f64) -> Vec<f64>;

    /// Fills `out` with `(feedback ordinal, answer)` for `set` against `guess`, see
    /// [`Pool::partition`].
    fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>);

    /// The allowed guesses after `guess` scored `ordinal`, see [`Allowed::after`].
    fn allowed_after(
        &self,
        allowed: &Allowed,
        mode: GuessPool,
        guess: u32,
        ordinal: u64,
    ) -> Allowed;

    /// Fills `table` for `set` and the guesses `allowed` permits.
    fn fill_table(&self, table: &mut PatternTable, set: &[u32], allowed: &Allowed);

    /// The `k` best-scored guesses for `set`, see [`Candidates::suggest_top_k`].
    fn top_k(&self, set: &[u32], strategy: &Strategy, allowed: &Allowed, k: usize) -> Vec<Scored>;

    /// The best-scored guess for `set`, see [`Candidates::suggest_index`].
    fn best_index(&self, set: &[u32], strategy: &Strategy, allowed: &Allowed) -> Option<u32>;
}

/// The pool shared by a decision tree, as the search sees it.
pub fn shared<const N: usize>(pool: &Arc<Pool<N>>) -> Arc<dyn PoolOps> {
    Arc::new(Arc::clone(pool))
}

impl<const N: usize> PoolOps for Arc<Pool<N>> {
    fn len(&self) -> usize {
        Pool::len(self)
    }

    fn indices(&self) -> Range<u32> {
        Pool::indices(self)
    }

    fn win_ordinal(&self) -> u64 {
        WordCorrectness::<N>::COUNT - 1
    }

    fn weights(&self, temperature: f64) -> Vec<f64> {
        Pool::weights(self, temperature)
    }

    fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>) {
        Pool::partition(self, set, guess, out);
    }

    fn allowed_after(
        &self,
        allowed: &Allowed,
        mode: GuessPool,
        guess: u32,
        ordinal: u64,
    ) -> Allowed {
        allowed.after(mode, self, self.code(guess), ordinal)
    }

    fn fill_table(&self, table: &mut PatternTable, set: &[u32], allowed: &Allowed) {
        table.build(self, set, allowed);
    }

    fn top_k(&self, set: &[u32], strategy: &Strategy, allowed: &Allowed, k: usize) -> Vec<Scored> {
        Candidates::from_indices(Self::clone(self), set.to_vec())
            .suggest_top_k(strategy, allowed, k)
    }

    fn best_index(&self, set: &[u32], strategy: &Strategy, allowed: &Allowed) -> Option<u32> {
        Candidates::from_indices(Self::clone(self), set.to_vec()).suggest_index(strategy, allowed)
    }
}
