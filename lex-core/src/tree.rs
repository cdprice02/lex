//! The Decision tree: one node per Guess path, each memoizing its suggestion.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use lex_data::{StoredGuess, Word};

use crate::allowed::Allowed;
use crate::book::{Listing, OpeningBook};
use crate::candidates::Candidates;
use crate::correctness::WordCorrectness;
use crate::endgame::Endgame;
use crate::explain::{Explainer, Explanation, LISTED, PatternShare, Record, ScoredGuess};
use crate::guesser::Guess;
use crate::policy::BasePolicy;
use crate::pool_ops::{self, PoolOps};
use crate::rollout::{ROLLOUT_MULTIPLIER, Rollout};
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
    /// The Strategy fixed for the tree's life and the Endgame whose memo of exact searches every
    /// game shares.
    base: BasePolicy,
    /// The Guess pool as the word-length-free search sees it.
    pool: Arc<dyn PoolOps>,
    /// Present when the strategy has a rollout width; owns the memo of base costs.
    rollout: Option<Rollout>,
    /// The nodes, root first.
    nodes: Vec<Node<N>>,
    /// Seeds nodes created along booked paths; `None` plays everything online.
    book: Option<OpeningBook<N>>,
    /// Whether a node records what it weighed when it decides; off, `suggest` runs the plain code.
    recording: bool,
    /// Scores guesses against the Answer prior; built when recording is switched on or a guess is
    /// first scored.
    explainer: Option<Explainer>,
    /// What each node recorded when it decided, kept beside the nodes so an unrecorded tree
    /// pays nothing per node.
    records: HashMap<NodeId, Record>,
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
        let pool = pool_ops::shared(&candidates.pool());
        let endgame = (strategy.endgame_threshold > 0).then(|| {
            Endgame::new(
                Arc::clone(&pool),
                strategy.pool,
                strategy.temperature,
                strategy.endgame_aversion,
            )
        });
        let rollout = (strategy.rollout_k > 0).then(|| Rollout::new(Arc::clone(&pool), &strategy));
        let allowed = Allowed::root(strategy.pool, candidates.len() == candidates.pool().len());
        let mut root = Node::new(candidates, allowed);
        let book = book.filter(|b| !b.is_empty());
        if let Some(book) = &book {
            Self::seed(&mut root, book, 0);
        }
        Self {
            base: BasePolicy::new(strategy, endgame),
            pool,
            rollout,
            nodes: vec![root],
            book,
            recording: false,
            explainer: None,
            records: HashMap::new(),
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
        self.base.strategy()
    }

    /// The suggestion at `node`, computed on first request.
    pub(crate) fn suggest(&mut self, node: NodeId) -> Option<Word<N>> {
        let Self {
            base,
            pool,
            rollout,
            nodes,
            explainer,
            records,
            recording,
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
            let set = entry.candidates.indices();
            let index = match explainer.as_ref().filter(|_| *recording) {
                Some(explainer) => explainer
                    .decide(set, &entry.allowed, base, rollout.as_mut())
                    .map(|record| {
                        let chosen = record.listed[0].index;
                        records.insert(node, record);
                        chosen
                    }),
                None => match rollout.as_mut() {
                    Some(rollout) if !base.uses_endgame(size) && rollout.applies(size) => {
                        rollout.best_guess(set, &entry.allowed, base)
                    }
                    _ => base.guess_index(pool, set, &entry.allowed),
                },
            };
            index.map(|i| entry.candidates.pool().word(i))
        };
        entry.suggestion = Memo::Computed(suggestion);
        suggestion
    }

    /// Switches recording on or off for the positions decided from now on.
    pub(crate) fn set_explain(&mut self, on: bool) {
        self.recording = on;
        if on {
            self.ensure_explainer();
        }
    }

    /// Builds the explainer once.
    fn ensure_explainer(&mut self) {
        if self.explainer.is_none() {
            let temperature = self.base.strategy().temperature;
            self.explainer = Some(Explainer::new(Arc::clone(&self.pool), temperature));
        }
    }

    /// The explanation of the choice at `node`, deciding it first if need be. `None` when
    /// recording is off, when no word is consistent, when the position was decided before
    /// recording was switched on, or when it is booked and the book holds no scores.
    pub(crate) fn explain(&mut self, node: NodeId) -> Option<Explanation<N>> {
        if !self.recording {
            return None;
        }
        let guess = self.suggest(node)?;
        let entry = &self.nodes[node];
        let pool = entry.candidates.pool();
        if let Some(record) = self.records.get(&node) {
            let permitted = |i| entry.allowed.permits(i, entry.candidates.indices());
            return Some(Explanation {
                guess,
                technique: record.technique,
                from_book: false,
                guesses: record
                    .listed
                    .iter()
                    .map(|l| ScoredGuess {
                        word: pool.word(l.index),
                        expected_bits: l.bits,
                        expected_remaining: l.remaining,
                        guess_cost: l.cost,
                        score: l.score,
                        in_candidates: l.in_candidates,
                        permitted: permitted(l.index),
                    })
                    .collect(),
                distribution: Self::shares(&record.distribution),
            });
        }
        let (book, on) = self.book.as_ref().zip(entry.book)?;
        let listing = book.listing(on)?;
        let explainer = self.explainer.as_ref()?;
        let set = entry.candidates.indices();
        Some(Explanation {
            guess,
            technique: listing.technique,
            from_book: true,
            guesses: listing
                .guesses
                .iter()
                .map(|g| ScoredGuess {
                    word: pool.word(g.index),
                    expected_bits: g.expected_bits,
                    expected_remaining: g.expected_remaining,
                    guess_cost: g.guess_cost,
                    score: g.score,
                    in_candidates: set.binary_search(&g.index).is_ok(),
                    permitted: entry.allowed.permits(g.index, set),
                })
                .collect(),
            distribution: Self::shares(&explainer.distribution(set, book.guess(on))),
        })
    }

    /// The public view of feedback groups.
    fn shares(groups: &[crate::explain::Group]) -> Vec<PatternShare<N>> {
        groups
            .iter()
            .map(|g| PatternShare {
                pattern: WordCorrectness::<N>::from_ordinal(g.ordinal),
                count: g.count,
                probability: g.probability,
            })
            .collect()
    }

    /// `word` scored against the candidates at `node`; `None` when it is not in the Guess pool.
    pub(crate) fn score(&mut self, node: NodeId, word: Word<N>) -> Option<ScoredGuess<N>> {
        self.ensure_explainer();
        let Self {
            base,
            rollout,
            nodes,
            explainer,
            ..
        } = self;
        let entry = &nodes[node];
        let pool = entry.candidates.pool();
        let index = pool.indices().find(|&i| pool.word(i) == word)?;
        let set = entry.candidates.indices();
        let listed = explainer
            .as_ref()?
            .score(set, &entry.allowed, base, rollout.as_mut(), index);
        Some(ScoredGuess {
            word,
            expected_bits: listed.bits,
            expected_remaining: listed.remaining,
            guess_cost: listed.cost,
            score: listed.score,
            in_candidates: listed.in_candidates,
            permitted: entry.allowed.permits(index, set),
        })
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
    ///
    /// Recording is on for the build, so each booked position keeps the technique and top guesses
    /// the one search that chose its guess already computed; it returns to its earlier setting.
    pub(crate) fn build_book(
        &mut self,
        floor: u64,
        progress: &mut dyn FnMut(usize),
    ) -> OpeningBook<N> {
        let pool = self.nodes[Self::ROOT].candidates.pool();
        let index_of: HashMap<Word<N>, u32> = pool.indices().map(|i| (pool.word(i), i)).collect();
        let mut book = OpeningBook::default();
        let was_recording = self.recording;
        self.set_explain(true);
        self.book_node(Self::ROOT, floor, &index_of, &mut book, progress);
        self.recording = was_recording;
        book
    }

    /// What `node` recorded when it decided; a position decided before recording was on is
    /// decided again from the warm memos, which reaches the same guess.
    fn record_at(&mut self, node: NodeId) -> Option<Record> {
        if let Some(record) = self.records.get(&node) {
            return Some(record.clone());
        }
        let Self {
            base,
            rollout,
            nodes,
            explainer,
            ..
        } = self;
        let entry = &nodes[node];
        explainer.as_ref()?.decide(
            entry.candidates.indices(),
            &entry.allowed,
            base,
            rollout.as_mut(),
        )
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
        let record = self.record_at(node)?;
        let chosen = index_of[&guess];
        debug_assert_eq!(
            record.listed[0].index, chosen,
            "the record's choice is the suggestion"
        );
        let listing = Listing {
            technique: record.technique,
            guesses: record
                .listed
                .iter()
                .take(LISTED)
                .map(|l| StoredGuess {
                    index: l.index,
                    expected_bits: l.bits,
                    expected_remaining: l.remaining,
                    guess_cost: l.cost,
                    score: l.score,
                })
                .collect(),
        };
        let id = book.push(chosen, listing);
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
            self.base.strategy().pool,
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
