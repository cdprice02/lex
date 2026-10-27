use std::collections::HashMap;

use lex_data::Word;

use crate::allowed::Allowed;
use crate::candidates::Candidates;
use crate::endgame::Endgame;
use crate::guesser::Guess;
use crate::strategy::Strategy;

pub(crate) type NodeId = usize;

/// Every game position reached so far, shared by all games played from one starting word set.
///
/// A position is identified by its guess path: the `(guess word, feedback ordinal)` steps from
/// the root. Given a fixed starting word set, the path fully determines the candidate set, and
/// the suggestion is a deterministic function of the candidate set and the [`Strategy`], which is
/// fixed for the tree's life, so two games that reach the same node need the suggestion computed
/// only once. The cache is only valid while
/// [`Candidates::suggest`] stays deterministic: randomness or hidden state there must come out
/// of the tree first.
///
/// The edge key carries the guess word because callers may push guesses that were not the
/// suggestion (the interactive assistant does).
#[derive(Debug)]
pub(crate) struct DecisionTree<const N: usize> {
    strategy: Strategy,
    /// Present when the strategy has an endgame threshold; owns the memo of exact searches.
    endgame: Option<Endgame<N>>,
    nodes: Vec<Node<N>>,
}

#[derive(Debug)]
struct Node<const N: usize> {
    candidates: Candidates<N>,
    /// The guesses the strategy's pool rule permits here; a function of the guess path.
    allowed: Allowed,
    suggestion: Option<Option<Word<N>>>,
    // Sparse: fan-out is 3^N per guess, 59,049 at the length cap.
    children: HashMap<(Word<N>, u64), NodeId>,
}

impl<const N: usize> Node<N> {
    fn new(candidates: Candidates<N>, allowed: Allowed) -> Self {
        Self {
            candidates,
            allowed,
            suggestion: None,
            children: HashMap::new(),
        }
    }
}

impl<const N: usize> DecisionTree<N> {
    pub(crate) const ROOT: NodeId = 0;

    pub(crate) fn new(candidates: Candidates<N>, strategy: Strategy) -> Self {
        let endgame = (strategy.endgame_threshold > 0).then(|| {
            Endgame::new(
                candidates.pool(),
                strategy.pool,
                strategy.temperature,
                strategy.endgame_aversion,
            )
        });
        let allowed = Allowed::root(strategy.pool, candidates.len() == candidates.pool().len());
        Self {
            strategy,
            endgame,
            nodes: vec![Node::new(candidates, allowed)],
        }
    }

    pub(crate) fn candidates(&self, node: NodeId) -> &Candidates<N> {
        &self.nodes[node].candidates
    }

    pub(crate) fn allowed(&self, node: NodeId) -> &Allowed {
        &self.nodes[node].allowed
    }

    pub(crate) fn strategy(&self) -> &Strategy {
        &self.strategy
    }

    /// The suggestion at `node`, computed on first request.
    pub(crate) fn suggest(&mut self, node: NodeId) -> Option<Word<N>> {
        let Self {
            strategy,
            endgame,
            nodes,
        } = self;
        let entry = &mut nodes[node];
        *entry.suggestion.get_or_insert_with(|| match endgame {
            // a lone candidate needs no search; `Candidates::suggest` returns it directly
            Some(endgame) if (2..=strategy.endgame_threshold).contains(&entry.candidates.len()) => {
                endgame.best_guess(entry.candidates.indices(), &entry.allowed)
            }
            _ => entry.candidates.suggest(strategy, &entry.allowed),
        })
    }

    /// The node reached from `node` by playing `guess`, created on first visit.
    pub(crate) fn child(&mut self, node: NodeId, guess: &Guess<N>) -> NodeId {
        let key = (guess.word(), guess.correctness().ordinal());
        if let Some(&id) = self.nodes[node].children.get(&key) {
            return id;
        }
        let parent = &self.nodes[node];
        let candidates = parent.candidates.filtered(guess);
        let pool = candidates.pool();
        let allowed = parent.allowed.after(
            self.strategy.pool,
            &pool,
            &pool.encode(&guess.word()),
            key.1,
        );
        let id = self.nodes.len();
        self.nodes.push(Node::new(candidates, allowed));
        self.nodes[node].children.insert(key, id);
        id
    }
}
