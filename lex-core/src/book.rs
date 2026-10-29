//! The Opening book: the persisted top levels of a Decision tree.

use std::marker::PhantomData;
use std::mem::size_of;

use lex_data::{StoredBook, StoredNode};
use thiserror::Error;

use crate::correctness::WordCorrectness;

/// Pattern evaluations (guesses scored times Candidate set size) above which a position is
/// booked. Calibrated once at about one second of single-core scoring on the reference machine
/// (`benchmarks/experiments/book-floor.md`) and never retuned: the solver digest in the book key
/// already rebuilds the book when the solver changes.
pub const BOOK_FLOOR: u64 = 40_000_000;

/// The top levels of a Decision tree, built ahead of play: for every position whose work exceeds
/// the floor, the guess the online search chose and the booked positions each feedback leads to.
///
/// Nodes are in preorder with the root first, so a child's id is always greater than its parent's.
/// A guess is a pool index and an edge key a feedback ordinal, so the book is meaningful only
/// against the pool, Answer list and Strategy it was built for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBook<const N: usize> {
    /// Booked positions in preorder.
    nodes: Vec<BookNode>,
    /// Ties the book to the word length it was built for.
    _word_length: PhantomData<[u8; N]>,
}

/// One booked position.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BookNode {
    /// Index into the Guess pool of the guess the online search chose here.
    guess: u32,
    /// Ascending by feedback ordinal; only to booked children.
    edges: Vec<(u64, u32)>,
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
                .map(|n| n.edges.capacity() * size_of::<(u64, u32)>())
                .sum::<usize>()
    }

    /// The book a [`StoredBook`] holds, checked against a pool of `pool_len` words: guesses are
    /// pool indices, children come after their parent, and ordinals are feedback patterns in
    /// ascending order. A book that fails was not built for this pool or has been damaged.
    pub fn from_nodes(stored: StoredBook, pool_len: usize) -> Result<Self, BookError> {
        let patterns = WordCorrectness::<N>::COUNT;
        let len = stored.nodes.len();
        for (node, stored_node) in stored.nodes.iter().enumerate() {
            if stored_node.guess as usize >= pool_len {
                return Err(BookError::GuessOutsidePool {
                    node,
                    guess: stored_node.guess,
                    pool_len,
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
                if child as usize <= node {
                    return Err(BookError::ChildNotAfterParent { node, child });
                }
                if child as usize >= len {
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
                })
                .collect(),
            _word_length: PhantomData,
        })
    }

    /// The stored form of this book, for `lex-data` to write.
    #[must_use]
    pub fn to_nodes(&self) -> StoredBook {
        StoredBook {
            nodes: self
                .nodes
                .iter()
                .map(|n| StoredNode {
                    guess: n.guess,
                    edges: n.edges.clone(),
                })
                .collect(),
        }
    }

    /// Appends a node and returns its id.
    pub(crate) fn push(&mut self, guess: u32) -> u32 {
        self.nodes.push(BookNode {
            guess,
            edges: Vec::new(),
        });
        (self.nodes.len() - 1) as u32
    }

    /// Edges must be added in ascending ordinal order.
    pub(crate) fn add_edge(&mut self, from: u32, ordinal: u64, to: u32) {
        let edges = &mut self.nodes[from as usize].edges;
        debug_assert!(edges.last().is_none_or(|&(o, _)| o < ordinal));
        edges.push((ordinal, to));
    }

    /// The pool index of the guess booked at `node`.
    pub(crate) fn guess(&self, node: u32) -> u32 {
        self.nodes[node as usize].guess
    }

    /// The booked position reached from `node` by feedback `ordinal`.
    pub(crate) fn edge(&self, node: u32, ordinal: u64) -> Option<u32> {
        let edges = &self.nodes[node as usize].edges;
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

    fn stored(nodes: &[(u32, &[(u64, u32)])]) -> StoredBook {
        StoredBook {
            nodes: nodes
                .iter()
                .map(|&(guess, edges)| StoredNode {
                    guess,
                    edges: edges.to_vec(),
                })
                .collect(),
        }
    }

    fn check(nodes: &[(u32, &[(u64, u32)])]) -> Result<OpeningBook<5>, BookError> {
        OpeningBook::from_nodes(stored(nodes), 10)
    }

    #[test]
    fn a_valid_book_round_trips_through_its_stored_form() {
        let stored = stored(&[(3, &[(1, 1), (242, 2)]), (4, &[(5, 2)]), (9, &[])]);
        let book = OpeningBook::<5>::from_nodes(stored.clone(), 10).unwrap();
        assert_eq!(book.len(), 3);
        assert_eq!(book.to_nodes(), stored);
        assert_eq!(
            OpeningBook::<5>::from_nodes(book.to_nodes(), 10).unwrap(),
            book
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
