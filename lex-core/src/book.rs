//! The Opening book: the persisted top levels of a Decision tree.

use std::marker::PhantomData;
use std::mem::size_of;

use lex_data::{StoredBook, StoredGuess, StoredNode};
use thiserror::Error;

use crate::correctness::WordCorrectness;
use crate::explain::{LISTED, Technique};

/// Pattern evaluations (guesses scored times Candidate set size) above which a position is booked.
///
/// Calibrated once at about one second of single-core scoring on the reference machine
/// (`benchmarks/experiments/book-floor.md`) and never retuned: the solver digest in the book key
/// already rebuilds the book when the solver changes.
pub const BOOK_FLOOR: u64 = 40_000_000;

/// The top levels of a Decision tree, built ahead of play: for every position whose work exceeds
/// the floor, the guess the online search chose and the booked positions each feedback leads to.
///
/// Nodes are in preorder with the root first, so a child's id is always greater than its parent's.
/// A guess is a pool index and an edge key a feedback ordinal, so the book is meaningful only
/// against the pool, Answer list and Strategy it was built for.
///
/// Each position also carries the technique and top guesses with their scores when the book was
/// built or loaded with scores; a book loaded for plain play drops them, so it holds only what
/// the play path reads.
#[derive(Debug, Clone, PartialEq)]
pub struct OpeningBook<const N: usize> {
    /// Booked positions in preorder.
    nodes: Vec<BookNode>,
    /// Ties the book to the word length it was built for.
    _word_length: PhantomData<[u8; N]>,
}

/// One booked position.
#[derive(Debug, Clone, PartialEq)]
struct BookNode {
    /// Index into the Guess pool of the guess the online search chose here.
    guess: u32,
    /// Ascending by feedback ordinal; only to booked children.
    edges: Vec<(u64, u32)>,
    /// What the search weighed, kept only when the book holds scores.
    listing: Option<Listing>,
}

/// The technique and top guesses the search recorded at a booked position.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    /// The method that chose the guess.
    pub technique: Technique,
    /// At most [`LISTED`] guesses, the chosen one first, with the scores the search computed.
    pub guesses: Vec<StoredGuess>,
}

/// Why a stored book is not a valid [`OpeningBook`] for its pool.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BookError {
    /// A node books a guess that is not an index into the pool.
    #[error("node {node} books guess {guess}, outside the pool of {pool_len} words")]
    GuessOutsidePool {
        /// Preorder index of the offending node.
        node: usize,
        /// The booked pool index.
        guess: u32,
        /// Number of words in the pool.
        pool_len: usize,
    },
    /// A node stores no guesses, or lists a different guess first than the one it books.
    #[error("node {node} does not list its booked guess first")]
    ChosenGuessNotFirst {
        /// Preorder index of the offending node.
        node: usize,
    },
    /// A node lists more guesses than a book keeps.
    #[error("node {node} lists {listed} guesses, more than the {LISTED} a book keeps")]
    TooManyListed {
        /// Preorder index of the offending node.
        node: usize,
        /// How many guesses it lists.
        listed: usize,
    },
    /// A node's technique is not one this build knows.
    #[error("node {node} names technique {code}, which this build does not know")]
    UnknownTechnique {
        /// Preorder index of the offending node.
        node: usize,
        /// The stored technique code.
        code: u8,
    },
    /// A node's child does not come after it in preorder.
    #[error("node {node} has child {child}, which does not come after it in preorder")]
    ChildNotAfterParent {
        /// Preorder index of the parent.
        node: usize,
        /// The child's preorder index.
        child: u32,
    },
    /// A node's child index is past the last node.
    #[error("node {node} has child {child}, but the book holds {len} nodes")]
    ChildOutsideBook {
        /// Preorder index of the parent.
        node: usize,
        /// The child's preorder index.
        child: u32,
        /// Number of nodes in the book.
        len: usize,
    },
    /// An edge key is not a feedback ordinal of the book's word length.
    #[error(
        "node {node} has feedback ordinal {ordinal}, outside the {patterns} patterns of length {n}"
    )]
    OrdinalOutsidePatterns {
        /// Preorder index of the offending node.
        node: usize,
        /// The edge's feedback ordinal.
        ordinal: u64,
        /// Number of feedback patterns, 3^`n`.
        patterns: u64,
        /// The word length.
        n: usize,
    },
    /// A node's edges are not strictly ascending by feedback ordinal.
    #[error("node {node} lists its feedback ordinals out of ascending order")]
    EdgesNotAscending {
        /// Preorder index of the offending node.
        node: usize,
    },
}

impl<const N: usize> Default for OpeningBook<N> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            _word_length: PhantomData,
        }
    }
}

impl<const N: usize> OpeningBook<N> {
    /// The number of booked positions.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether no position is booked, so every position is searched online.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Bytes the book holds in memory, counting toward the 1 GB cap.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        size_of::<Self>()
            + self.nodes.capacity() * size_of::<BookNode>()
            + self
                .nodes
                .iter()
                .map(|n| {
                    n.edges.capacity() * size_of::<(u64, u32)>()
                        + n.listing
                            .as_ref()
                            .map_or(0, |l| l.guesses.capacity() * size_of::<StoredGuess>())
                })
                .sum::<usize>()
    }

    /// The book a [`StoredBook`] holds, checked against a pool of `pool_len` words: guesses are
    /// pool indices, children come after their parent, and ordinals are feedback patterns in
    /// ascending order, and each node lists its booked guess first, at most 10 guesses and
    /// a known technique. A book that fails was not built for this pool or has been damaged.
    ///
    /// With `keep_scores` the technique and listed guesses stay in the book so an explanation can
    /// read them; without it they are dropped here and the book is what plain play needs.
    ///
    /// # Errors
    ///
    /// A [`BookError`] naming the first check the book fails.
    pub fn from_nodes(
        stored: StoredBook,
        pool_len: usize,
        keep_scores: bool,
    ) -> Result<Self, BookError> {
        let patterns = WordCorrectness::<N>::COUNT;
        let len = stored.nodes.len();
        for (node, stored_node) in stored.nodes.iter().enumerate() {
            if usize::try_from(stored_node.guess).unwrap_or(usize::MAX) >= pool_len {
                return Err(BookError::GuessOutsidePool {
                    node,
                    guess: stored_node.guess,
                    pool_len,
                });
            }
            let outside = |index: u32| usize::try_from(index).unwrap_or(usize::MAX) >= pool_len;
            if let Some(bad) = stored_node.guesses.iter().find(|g| outside(g.index)) {
                return Err(BookError::GuessOutsidePool {
                    node,
                    guess: bad.index,
                    pool_len,
                });
            }
            if stored_node.guesses.first().map(|g| g.index) != Some(stored_node.guess) {
                return Err(BookError::ChosenGuessNotFirst { node });
            }
            if stored_node.guesses.len() > LISTED {
                return Err(BookError::TooManyListed {
                    node,
                    listed: stored_node.guesses.len(),
                });
            }
            if Technique::from_code(stored_node.technique).is_none() {
                return Err(BookError::UnknownTechnique {
                    node,
                    code: stored_node.technique,
                });
            }
            for (i, &(ordinal, child)) in stored_node.edges.iter().enumerate() {
                if ordinal >= patterns {
                    return Err(BookError::OrdinalOutsidePatterns {
                        node,
                        ordinal,
                        patterns,
                        n: N,
                    });
                }
                if i > 0 && stored_node.edges[i - 1].0 >= ordinal {
                    return Err(BookError::EdgesNotAscending { node });
                }
                let child_at = usize::try_from(child).unwrap_or(usize::MAX);
                if child_at <= node {
                    return Err(BookError::ChildNotAfterParent { node, child });
                }
                if child_at >= len {
                    return Err(BookError::ChildOutsideBook { node, child, len });
                }
            }
        }
        Ok(Self {
            nodes: stored
                .nodes
                .into_iter()
                .map(|n| BookNode {
                    guess: n.guess,
                    edges: n.edges,
                    listing: keep_scores
                        .then(|| {
                            Technique::from_code(n.technique).map(|technique| Listing {
                                technique,
                                guesses: n.guesses,
                            })
                        })
                        .flatten(),
                })
                .collect(),
            _word_length: PhantomData,
        })
    }

    /// The stored form of this book, for `lex-data` to write; `None` when the book holds no
    /// scores, since a file always stores them.
    #[must_use]
    pub fn to_nodes(&self) -> Option<StoredBook> {
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                let listing = n.listing.as_ref()?;
                Some(StoredNode {
                    guess: n.guess,
                    technique: listing.technique.code(),
                    guesses: listing.guesses.clone(),
                    edges: n.edges.clone(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(StoredBook { nodes })
    }

    /// This book with every position's technique and scores dropped: the play path reads only
    /// guesses and edges.
    #[must_use]
    pub fn without_scores(mut self) -> Self {
        for node in &mut self.nodes {
            node.listing = None;
        }
        self
    }

    /// Whether every position holds its technique and scores (true of an empty book).
    #[must_use]
    pub fn has_scores(&self) -> bool {
        self.nodes.iter().all(|n| n.listing.is_some())
    }

    /// The technique and top guesses booked at `node`, when the book holds scores.
    pub(crate) fn listing(&self, node: u32) -> Option<&Listing> {
        self.nodes[usize::try_from(node).unwrap_or(usize::MAX)]
            .listing
            .as_ref()
    }

    /// Appends a node and returns its id.
    #[expect(
        clippy::expect_used,
        reason = "a book holds one in-memory node per booked position, so 2^32 of them would need over 100 GiB first"
    )]
    pub(crate) fn push(&mut self, guess: u32, listing: Listing) -> u32 {
        let id = u32::try_from(self.nodes.len()).expect("the book exceeds u32::MAX nodes");
        self.nodes.push(BookNode {
            guess,
            edges: Vec::new(),
            listing: Some(listing),
        });
        id
    }

    /// Edges must be added in ascending ordinal order.
    pub(crate) fn add_edge(&mut self, from: u32, ordinal: u64, to: u32) {
        let edges = &mut self.nodes[usize::try_from(from).unwrap_or(usize::MAX)].edges;
        debug_assert!(
            edges.last().is_none_or(|&(o, _)| o < ordinal),
            "edges must be added in ascending ordinal order"
        );
        edges.push((ordinal, to));
    }

    /// The pool index of the guess booked at `node`.
    pub(crate) fn guess(&self, node: u32) -> u32 {
        self.nodes[usize::try_from(node).unwrap_or(usize::MAX)].guess
    }

    /// The booked position reached from `node` by feedback `ordinal`.
    pub(crate) fn edge(&self, node: u32, ordinal: u64) -> Option<u32> {
        let edges = &self.nodes[usize::try_from(node).unwrap_or(usize::MAX)].edges;
        edges
            .binary_search_by_key(&ordinal, |&(o, _)| o)
            .ok()
            .map(|i| edges[i].1)
    }

    /// The largest booked Guess pool index, or `None` for an empty book.
    pub(crate) fn max_guess(&self) -> Option<u32> {
        self.nodes.iter().map(|n| n.guess).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(index: u32, bits: f64) -> StoredGuess {
        StoredGuess {
            index,
            expected_bits: bits,
            expected_remaining: 1.0 / bits,
            guess_cost: (index.is_multiple_of(2)).then_some(bits + 1.0),
            score: bits + 0.1,
        }
    }

    fn stored(nodes: &[(u32, &[(u64, u32)])]) -> StoredBook {
        StoredBook {
            nodes: nodes
                .iter()
                .map(|&(guess, edges)| StoredNode {
                    guess,
                    technique: Technique::Rollout.code(),
                    guesses: vec![scored(guess, 2.5), scored((guess + 1) % 10, 1.5)],
                    edges: edges.to_vec(),
                })
                .collect(),
        }
    }

    fn check(nodes: &[(u32, &[(u64, u32)])]) -> Result<OpeningBook<5>, BookError> {
        OpeningBook::from_nodes(stored(nodes), 10, true)
    }

    fn check_node(edit: impl FnOnce(&mut StoredNode)) -> Result<OpeningBook<5>, BookError> {
        let mut book = stored(&[(3, &[])]);
        edit(&mut book.nodes[0]);
        OpeningBook::from_nodes(book, 10, true)
    }

    #[test]
    fn a_valid_book_round_trips_through_its_stored_form() {
        let stored = stored(&[(3, &[(1, 1), (242, 2)]), (4, &[(5, 2)]), (9, &[])]);
        let book = OpeningBook::<5>::from_nodes(stored.clone(), 10, true).unwrap();
        assert_eq!(book.len(), 3);
        assert!(book.has_scores());
        assert_eq!(book.to_nodes().as_ref(), Some(&stored));
        assert_eq!(
            OpeningBook::<5>::from_nodes(book.to_nodes().unwrap(), 10, true).unwrap(),
            book
        );
        let listing = book.listing(1).unwrap();
        assert_eq!(listing.technique, Technique::Rollout);
        assert_eq!(listing.guesses, stored.nodes[1].guesses);
    }

    #[test]
    fn loading_without_scores_keeps_the_play_path_and_drops_the_listings() {
        let stored = stored(&[(3, &[(1, 1)]), (4, &[])]);
        let plain = OpeningBook::<5>::from_nodes(stored.clone(), 10, false).unwrap();
        let full = OpeningBook::<5>::from_nodes(stored, 10, true).unwrap();
        assert!(!plain.has_scores());
        assert!(plain.listing(0).is_none());
        assert_eq!(plain.to_nodes(), None);
        assert_eq!(plain.guess(0), full.guess(0));
        assert_eq!(plain.edge(0, 1), full.edge(0, 1));
        assert_eq!(full.without_scores(), plain);
        assert!(
            plain.heap_bytes()
                < OpeningBook::<5>::from_nodes(stored_again(), 10, true)
                    .unwrap()
                    .heap_bytes()
        );
    }

    fn stored_again() -> StoredBook {
        stored(&[(3, &[(1, 1)]), (4, &[])])
    }

    #[test]
    fn a_node_must_list_its_booked_guess_first_within_the_limit_with_a_known_technique() {
        assert_eq!(
            check_node(|n| n.guesses.clear()),
            Err(BookError::ChosenGuessNotFirst { node: 0 })
        );
        assert_eq!(
            check_node(|n| n.guesses.reverse()),
            Err(BookError::ChosenGuessNotFirst { node: 0 })
        );
        assert_eq!(
            check_node(|n| n.guesses = (0..=10)
                .map(|i| scored(if i == 0 { 3 } else { i % 10 }, 1.0))
                .collect()),
            Err(BookError::TooManyListed {
                node: 0,
                listed: 11
            })
        );
        assert!(
            check_node(|n| n.guesses = (0..10)
                .map(|i| scored(if i == 0 { 3 } else { i % 10 }, 1.0))
                .collect())
            .is_ok()
        );
        assert_eq!(
            check_node(|n| n.technique = 9),
            Err(BookError::UnknownTechnique { node: 0, code: 9 })
        );
        assert_eq!(
            check_node(|n| n.guesses.push(scored(10, 1.0))),
            Err(BookError::GuessOutsidePool {
                node: 0,
                guess: 10,
                pool_len: 10
            })
        );
    }

    #[test]
    fn an_empty_book_is_valid() {
        assert!(check(&[]).unwrap().is_empty());
    }

    #[test]
    fn a_guess_outside_the_pool_is_rejected() {
        assert_eq!(
            check(&[(10, &[])]),
            Err(BookError::GuessOutsidePool {
                node: 0,
                guess: 10,
                pool_len: 10
            })
        );
    }

    #[test]
    fn a_child_must_come_after_its_parent_inside_the_book() {
        assert_eq!(
            check(&[(0, &[(1, 0)])]),
            Err(BookError::ChildNotAfterParent { node: 0, child: 0 })
        );
        assert_eq!(
            check(&[(0, &[(1, 1)]), (0, &[(1, 1)])]),
            Err(BookError::ChildNotAfterParent { node: 1, child: 1 })
        );
        assert_eq!(
            check(&[(0, &[(1, 5)])]),
            Err(BookError::ChildOutsideBook {
                node: 0,
                child: 5,
                len: 1
            })
        );
    }

    #[test]
    fn an_ordinal_must_be_a_pattern_of_the_word_length() {
        assert!(check(&[(0, &[(242, 1)]), (0, &[])]).is_ok());
        assert_eq!(
            check(&[(0, &[(243, 1)]), (0, &[])]),
            Err(BookError::OrdinalOutsidePatterns {
                node: 0,
                ordinal: 243,
                patterns: 243,
                n: 5
            })
        );
    }

    #[test]
    fn ordinals_must_ascend_strictly() {
        for edges in [&[(5, 1), (4, 1)][..], &[(4, 1), (4, 1)]] {
            assert_eq!(
                check(&[(0, edges), (0, &[])]),
                Err(BookError::EdgesNotAscending { node: 0 })
            );
        }
    }
}
