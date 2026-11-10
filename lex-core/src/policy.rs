//! The solver's choice at a position without rollout.

use std::sync::Arc;

use crate::allowed::Allowed;
use crate::endgame::Endgame;
use crate::pool_ops::PoolOps;
use crate::strategy::Strategy;

/// The Base policy: the Endgame for 2 to `endgame_threshold` candidates, otherwise the best
/// entropy-scored guess. The Endgame lives here, not in [`Strategy`], because the Strategy is a
/// `Copy` value in the book key and the Endgame is mutable memo state.
#[derive(Debug)]
pub struct BasePolicy {
    /// The Strategy the tree scores under, fixed for the policy's life.
    strategy: Strategy,
    /// The exact search; present when the Strategy has an Endgame threshold.
    endgame: Option<Endgame>,
}

impl BasePolicy {
    /// A policy under `strategy` that searches with `endgame` within its threshold.
    pub(crate) const fn new(strategy: Strategy, endgame: Option<Endgame>) -> Self {
        Self { strategy, endgame }
    }

    /// The Strategy this policy scores under.
    pub(crate) const fn strategy(&self) -> &Strategy {
        &self.strategy
    }

    /// Whether the Endgame chooses the guess for a Candidate set of `size`. A lone candidate
    /// needs no search: greedy returns it directly.
    pub(crate) fn uses_endgame(&self, size: usize) -> bool {
        self.endgame.is_some() && (2..=self.strategy.endgame_threshold).contains(&size)
    }

    /// The guess for the candidate `set` (ascending pool indices) of `pool`, as a pool index.
    pub(crate) fn guess_index(
        &mut self,
        pool: &Arc<dyn PoolOps>,
        set: &[u32],
        allowed: &Allowed,
    ) -> Option<u32> {
        if self.uses_endgame(set.len())
            && let Some(endgame) = self.endgame.as_mut()
        {
            return endgame.best_index(set, allowed);
        }
        pool.best_index(set, &self.strategy, allowed)
    }
}

impl BasePolicy {
    /// The Endgame's guess for `set` and its Guess cost, memoized with the guess itself.
    pub(crate) fn endgame_pick(&mut self, set: &[u32], allowed: &Allowed) -> Option<(u32, f64)> {
        self.endgame.as_mut()?.best_with_cost(set, allowed)
    }

    /// The Guess cost of playing `guess` now at an Endgame position, the Endgame's own after.
    pub(crate) fn endgame_cost(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        guess: u32,
    ) -> Option<f64> {
        Some(self.endgame.as_mut()?.cost_of(set, allowed, guess))
    }
}
