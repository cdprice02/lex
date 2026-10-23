use std::collections::HashMap;

use lex_data::Word;

use crate::candidates::Candidates;
use crate::guesser::Guess;

pub(crate) type NodeId = usize;

/// Every game position reached so far, shared by all games played from one starting word set.
///
/// A position is identified by its guess path: the `(guess word, feedback ordinal)` steps from
/// the root. Given a fixed starting word set, the path fully determines the candidate set, and
/// the suggestion is a deterministic function of the candidate set, so two games that reach the
/// same node need the suggestion computed only once. The cache is only valid while
/// [`Candidates::suggest`] stays deterministic: randomness or hidden state there must come out
/// of the tree first.
///
/// The edge key carries the guess word because callers may push guesses that were not the
/// suggestion (the interactive assistant does).
#[derive(Debug)]
pub(crate) struct DecisionTree<const N: usize> {
    nodes: Vec<Node<N>>,
}

#[derive(Debug)]
struct Node<const N: usize> {
    candidates: Candidates<N>,
    suggestion: Option<Option<Word<N>>>,
    // Sparse: fan-out is 3^N per guess, 59,049 at the length cap.
    children: HashMap<(Word<N>, u16), NodeId>,
}

impl<const N: usize> Node<N> {
    fn new(candidates: Candidates<N>) -> Self {
        Self {
            candidates,
            suggestion: None,
            children: HashMap::new(),
        }
    }
}

impl<const N: usize> DecisionTree<N> {
    pub(crate) const ROOT: NodeId = 0;

    pub(crate) fn new(candidates: Candidates<N>) -> Self {
        Self {
            nodes: vec![Node::new(candidates)],
        }
    }

    pub(crate) fn candidates(&self, node: NodeId) -> &Candidates<N> {
        &self.nodes[node].candidates
    }

    /// The suggestion at `node`, computed on first request.
    pub(crate) fn suggest(&mut self, node: NodeId) -> Option<Word<N>> {
        let entry = &mut self.nodes[node];
        *entry
            .suggestion
            .get_or_insert_with(|| entry.candidates.suggest())
    }

    /// The node reached from `node` by playing `guess`, created on first visit.
    pub(crate) fn child(&mut self, node: NodeId, guess: &Guess<N>) -> NodeId {
        let key = (guess.word(), guess.correctness().ordinal_u16());
        if let Some(&id) = self.nodes[node].children.get(&key) {
            return id;
        }
        let candidates = self.nodes[node].candidates.filtered(guess);
        let id = self.nodes.len();
        self.nodes.push(Node::new(candidates));
        self.nodes[node].children.insert(key, id);
        id
    }
}
