//! The solver's choice at a position without rollout.

use lex_data::Word;

use crate::allowed::Allowed;
use crate::candidates::Candidates;
use crate::endgame::Endgame;
use crate::strategy::Strategy;

/// The Base policy: the Endgame for 2 to `endgame_threshold` candidates, otherwise the best
/// entropy-scored guess. The Endgame lives here, not in [`Strategy`], because the Strategy is a
/// `Copy` value in the book key and the Endgame is mutable memo state.
#[derive(Debug)]
pub struct BasePolicy<const N: usize> {
    /// The Strategy the tree scores under, fixed for the policy's life.
    strategy: Strategy,
    /// The exact search; present when the Strategy has an Endgame threshold.
    endgame: Option<Endgame<N>>,
}

impl<const N: usize> BasePolicy<N> {
    /// A policy under `strategy` that searches with `endgame` within its threshold.
    pub(crate) const fn new(strategy: Strategy, endgame: Option<Endgame<N>>) -> Self {
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

    /// The guess for `candidates` as a pool index.
    pub(crate) fn guess_index(
        &mut self,
        candidates: &Candidates<N>,
        allowed: &Allowed,
    ) -> Option<u32> {
        if self.uses_endgame(candidates.len())
            && let Some(endgame) = self.endgame.as_mut()
        {
            return endgame.best_index(candidates.indices(), allowed);
        }
        candidates.suggest_index(&self.strategy, allowed)
    }

    /// [`guess_index`](Self::guess_index) as a word.
    pub(crate) fn guess(
        &mut self,
        candidates: &Candidates<N>,
        allowed: &Allowed,
    ) -> Option<Word<N>> {
        let pool = candidates.pool();
        self.guess_index(candidates, allowed).map(|i| pool.word(i))
    }
}
