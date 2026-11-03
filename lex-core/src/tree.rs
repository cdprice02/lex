//! The Decision tree: one node per Guess path, each memoizing its suggestion.

use std::collections::{BTreeSet, HashMap};

use lex_data::Word;

use crate::allowed::{Allowed, AllowedIds};
use crate::book::OpeningBook;
use crate::candidates::Candidates;
use crate::correctness::WordCorrectness;
use crate::endgame::Endgame;
use crate::guesser::Guess;
use crate::rollout::{Base, ROLLOUT_MULTIPLIER, Rollout};
use crate::strategy::Strategy;

/// Index of a node in the tree's arena.
pub type NodeId = usize;

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
pub struct DecisionTree<const N: usize> {
    /// The Strategy fixed for the tree's life.
    strategy: Strategy,
    /// Present when the strategy has an endgame threshold; owns the memo of exact searches.
    endgame: Option<Endgame<N>>,
    /// Present when the strategy has a rollout width; owns the memo of base costs.
    rollout: Option<Rollout<N>>,
    /// The nodes, root first.
    nodes: Vec<Node<N>>,
    /// Seeds nodes created along booked paths; `None` plays everything online.
    book: Option<OpeningBook<N>>,
    /// Suggestions computed at a booked node, which a correctly seeded run never needs.
    #[cfg(test)]
    booked_computations: usize,
}

/// A node's memoized suggestion.
#[derive(Debug, Clone, Copy)]
enum Memo<const N: usize> {
    /// Not yet computed.
    Pending,
    /// Computed; `None` when no consistent word remains.
    Computed(Option<Word<N>>),
}

/// One game position.
#[derive(Debug)]
struct Node<const N: usize> {
    /// The words still consistent with the Guess path.
    candidates: Candidates<N>,
    /// The guesses the strategy's pool rule permits here; a function of the guess path.
    allowed: Allowed,
    /// The memoized suggestion.
    suggestion: Memo<N>,
    /// The book node this position is on, while the game stays on the book's path.
    book: Option<u32>,
    // Sparse: fan-out is 3^N per guess, 59,049 at the length cap.
    /// The positions reached from here, by the (guess, feedback ordinal) step taken.
    children: HashMap<(Word<N>, u64), NodeId>,
}

impl<const N: usize> Node<N> {
    /// A node holding `candidates`, with nothing computed yet.
    fn new(candidates: Candidates<N>, allowed: Allowed) -> Self {
        Self {
            candidates,
            allowed,
            suggestion: Memo::Pending,
            book: None,
            children: HashMap::new(),
        }
    }
}

impl<const N: usize> DecisionTree<N> {
    /// The node of the starting position.
    pub(crate) const ROOT: NodeId = 0;

    /// A tree rooted at the full Candidate set, optionally seeded from `book`.
    pub(crate) fn new(
        candidates: Candidates<N>,
        strategy: Strategy,
        book: Option<OpeningBook<N>>,
    ) -> Self {
        let list_ids = AllowedIds::default();
        let endgame = (strategy.endgame_threshold > 0).then(|| {
            Endgame::new(
                candidates.pool(),
                strategy.pool,
                strategy.temperature,
                strategy.endgame_aversion,
            )
            .sharing(list_ids.clone())
        });
        let rollout =
            (strategy.rollout_k > 0).then(|| Rollout::new(candidates.pool(), &strategy, list_ids));
        let allowed = Allowed::root(strategy.pool, candidates.len() == candidates.pool().len());
        let mut root = Node::new(candidates, allowed);
        let book = book.filter(|b| !b.is_empty());
        if let Some(book) = &book {
            Self::seed(&mut root, book, 0);
        }
        Self {
            strategy,
            endgame,
            rollout,
            nodes: vec![root],
            book,
            #[cfg(test)]
            booked_computations: 0,
        }
    }

    /// Puts `node` on book node `id` and takes the booked guess as its suggestion.
    fn seed(node: &mut Node<N>, book: &OpeningBook<N>, id: u32) {
        node.book = Some(id);
        node.suggestion = Memo::Computed(Some(node.candidates.pool().word(book.guess(id))));
    }

    /// The Candidate set at `node`.
    pub(crate) fn candidates(&self, node: NodeId) -> &Candidates<N> {
        &self.nodes[node].candidates
    }

    /// The guesses permitted at `node`.
    pub(crate) fn allowed(&self, node: NodeId) -> &Allowed {
        &self.nodes[node].allowed
    }

    /// The Strategy this tree scores under.
    pub(crate) const fn strategy(&self) -> &Strategy {
        &self.strategy
    }

    /// The suggestion at `node`, computed on first request.
    pub(crate) fn suggest(&mut self, node: NodeId) -> Option<Word<N>> {
        let Self {
            strategy,
            endgame,
            rollout,
            nodes,
            #[cfg(test)]
            booked_computations,
            ..
        } = self;
        let entry = &mut nodes[node];
        if let Memo::Computed(suggestion) = entry.suggestion {
            return suggestion;
        }
        let suggestion = {
            #[cfg(test)]
            if entry.book.is_some() {
                *booked_computations += 1;
            }
            let size = entry.candidates.len();
            match (endgame.as_mut(), rollout.as_mut()) {
                // a lone candidate needs no search; `Candidates::suggest` returns it directly
                (Some(endgame), _) if (2..=strategy.endgame_threshold).contains(&size) => {
                    endgame.best_guess(entry.candidates.indices(), &entry.allowed)
                }
                (_, Some(rollout)) if rollout.applies(size) => rollout.best_guess(
                    &entry.candidates,
                    &entry.allowed,
                    &mut Base { strategy, endgame },
                ),
                _ => entry.candidates.suggest(strategy, &entry.allowed),
            }
        };
        entry.suggestion = Memo::Computed(suggestion);
        suggestion
    }

    /// Pattern evaluations of one scan at `node`: the guesses its rule allows times its candidates.
    fn scan_evaluations(&self, node: NodeId) -> u64 {
        let entry = &self.nodes[node];
        let size = entry.candidates.len();
        let allowed = entry
            .allowed
            .guess_count(entry.candidates.pool().len(), size);
        let widen = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        widen(allowed).saturating_mul(widen(size))
    }

    /// Work at `node`: one scan, scaled up where a rollout chooses the guess.
    fn work(&self, node: NodeId) -> u64 {
        let evaluations = self.scan_evaluations(node);
        let size = self.nodes[node].candidates.len();
        if self.rollout.as_ref().is_some_and(|r| r.applies(size)) {
            evaluations.saturating_mul(ROLLOUT_MULTIPLIER)
        } else {
            evaluations
        }
    }

    /// Moves the lower edge of the rollout band to above `floor` candidates.
    #[cfg(test)]
    pub(crate) fn set_rollout_floor(&mut self, floor: usize) {
        self.rollout = self.rollout.take().map(|r| r.with_floor(floor));
    }

    /// What the rollouts run so far spent, and what plain scans of the same positions would have:
    /// the measured multiplier is the first over the second.
    #[cfg(test)]
    pub(crate) fn rollout_cost(&self) -> (u64, u64) {
        let Some(rollout) = &self.rollout else {
            return (0, 0);
        };
        let scans = (0..self.nodes.len())
            .filter(|&n| {
                matches!(self.nodes[n].suggestion, Memo::Computed(_))
                    && rollout.applies(self.nodes[n].candidates.len())
            })
            .map(|n| self.scan_evaluations(n))
            .sum();
        (rollout.evaluations() + scans, scans)
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
        let index_of: HashMap<Word<N>, u32> = pool.indices().map(|i| (pool.word(i), i)).collect();
        let mut book = OpeningBook::default();
        self.book_node(Self::ROOT, floor, &index_of, &mut book, progress);
        book
    }

    /// Books `node` and its booked descendants into `book`, returning its book id, or `None` when its work is at or below `floor`.
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
    pub(crate) const fn booked_computations(&self) -> usize {
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
