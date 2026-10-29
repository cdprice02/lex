use std::collections::{BTreeSet, HashMap};

use lex_data::Word;

use crate::allowed::Allowed;
use crate::book::OpeningBook;
use crate::candidates::Candidates;
use crate::correctness::WordCorrectness;
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
    /// Seeds nodes created along booked paths; `None` plays everything online.
    book: Option<OpeningBook<N>>,
    /// Suggestions computed at a booked node, which a correctly seeded run never needs.
    #[cfg(test)]
    booked_computations: usize,
}

#[derive(Debug)]
struct Node<const N: usize> {
    candidates: Candidates<N>,
    /// The guesses the strategy's pool rule permits here; a function of the guess path.
    allowed: Allowed,
    suggestion: Option<Option<Word<N>>>,
    /// The book node this position is on, while the game stays on the book's path.
    book: Option<u32>,
    // Sparse: fan-out is 3^N per guess, 59,049 at the length cap.
    children: HashMap<(Word<N>, u64), NodeId>,
}

impl<const N: usize> Node<N> {
    fn new(candidates: Candidates<N>, allowed: Allowed) -> Self {
        Self {
            candidates,
            allowed,
            suggestion: None,
            book: None,
            children: HashMap::new(),
        }
    }
}

impl<const N: usize> DecisionTree<N> {
    pub(crate) const ROOT: NodeId = 0;

    pub(crate) fn new(
        candidates: Candidates<N>,
        strategy: Strategy,
        book: Option<OpeningBook<N>>,
    ) -> Self {
        let endgame = (strategy.endgame_threshold > 0).then(|| {
            Endgame::new(
                candidates.pool(),
                strategy.pool,
                strategy.temperature,
                strategy.endgame_aversion,
            )
        });
        let allowed = Allowed::root(strategy.pool, candidates.len() == candidates.pool().len());
        let mut root = Node::new(candidates, allowed);
        let book = book.filter(|b| !b.is_empty());
        if let Some(book) = &book {
            Self::seed(&mut root, book, 0);
        }
        Self {
            strategy,
            endgame,
            nodes: vec![root],
            book,
            #[cfg(test)]
            booked_computations: 0,
        }
    }

    /// Puts `node` on book node `id` and takes the booked guess as its suggestion.
    fn seed(node: &mut Node<N>, book: &OpeningBook<N>, id: u32) {
        node.book = Some(id);
        node.suggestion = Some(Some(node.candidates.pool().word(book.guess(id))));
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
            #[cfg(test)]
            booked_computations,
            ..
        } = self;
        let entry = &mut nodes[node];
        *entry.suggestion.get_or_insert_with(|| {
            #[cfg(test)]
            if entry.book.is_some() {
                *booked_computations += 1;
            }
            match endgame {
                // a lone candidate needs no search; `Candidates::suggest` returns it directly
                Some(endgame)
                    if (2..=strategy.endgame_threshold).contains(&entry.candidates.len()) =>
                {
                    endgame.best_guess(entry.candidates.indices(), &entry.allowed)
                }
                _ => entry.candidates.suggest(strategy, &entry.allowed),
            }
        })
    }

    /// Work at `node`: the guesses its rule allows times its candidates.
    fn work(&self, node: NodeId) -> u64 {
        let entry = &self.nodes[node];
        let allowed = match &entry.allowed {
            Allowed::Whole => entry.candidates.pool().len(),
            Allowed::Candidates => entry.candidates.len(),
            Allowed::List(list) => list.len(),
        };
        allowed as u64 * entry.candidates.len() as u64
    }

    /// Books every position, top-down, whose work exceeds `floor`. Work never rises along a
    /// path, so the booked positions form a subtree rooted at the start.
    /// `progress` is called with the booked count each time a position is booked.
    pub(crate) fn build_book(
        &mut self,
        floor: u64,
        progress: &mut dyn FnMut(usize),
    ) -> OpeningBook<N> {
        let pool = self.nodes[Self::ROOT].candidates.pool();
        let index_of: HashMap<Word<N>, u32> =
            (0..pool.len() as u32).map(|i| (pool.word(i), i)).collect();
        let mut book = OpeningBook::default();
        self.book_node(Self::ROOT, floor, &index_of, &mut book, progress);
        book
    }

    fn book_node(
        &mut self,
        node: NodeId,
        floor: u64,
        index_of: &HashMap<Word<N>, u32>,
        book: &mut OpeningBook<N>,
        progress: &mut dyn FnMut(usize),
    ) -> Option<u32> {
        if self.work(node) <= floor {
            return None;
        }
        let guess = self.suggest(node)?;
        let id = book.push(index_of[&guess]);
        progress(book.len());
        let pool = self.nodes[node].candidates.pool();
        let ordinals: BTreeSet<u64> = self.nodes[node]
            .candidates
            .indices()
            .iter()
            .map(|&i| WordCorrectness::correct(pool.word(i), guess).ordinal())
            .collect();
        for ordinal in ordinals {
            let feedback = WordCorrectness::<N>::from_ordinal(ordinal);
            if feedback.is_correct() {
                continue;
            }
            let child = self.child(node, &Guess::new(guess, feedback));
            if let Some(booked) = self.book_node(child, floor, index_of, book, progress) {
                book.add_edge(id, ordinal, booked);
            }
        }
        Some(id)
    }

    #[cfg(test)]
    pub(crate) fn booked_computations(&self) -> usize {
        self.booked_computations
    }

    /// The node reached from `node` by playing `guess`, created on first visit.
    pub(crate) fn child(&mut self, node: NodeId, guess: &Guess<N>) -> NodeId {
        let key = (guess.word(), guess.correctness().ordinal());
        if let Some(&id) = self.nodes[node].children.get(&key) {
            return id;
        }
        let parent = &self.nodes[node];
        let booked = self.book.as_ref().and_then(|book| {
            let on = parent.book?;
            let pool = parent.candidates.pool();
            (pool.word(book.guess(on)) == guess.word())
                .then(|| book.edge(on, key.1))
                .flatten()
        });
        let candidates = parent.candidates.filtered(guess);
        let pool = candidates.pool();
        let allowed = parent.allowed.after(
            self.strategy.pool,
            &pool,
            &pool.encode(&guess.word()),
            key.1,
        );
        let id = self.nodes.len();
        let mut child = Node::new(candidates, allowed);
        if let (Some(book), Some(on)) = (&self.book, booked) {
            Self::seed(&mut child, book, on);
        }
        self.nodes.push(child);
        self.nodes[node].children.insert(key, id);
        id
    }
}
