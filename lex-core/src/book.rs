use std::marker::PhantomData;
use std::mem::size_of;

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
    nodes: Vec<BookNode>,
    _word_length: PhantomData<[u8; N]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BookNode {
    guess: u32,
    /// Ascending by feedback ordinal; only to booked children.
    edges: Vec<(u64, u32)>,
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
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Bytes the book holds in memory, counting toward the 1 GB cap.
    pub fn heap_bytes(&self) -> usize {
        size_of::<Self>()
            + self.nodes.capacity() * size_of::<BookNode>()
            + self
                .nodes
                .iter()
                .map(|n| n.edges.capacity() * size_of::<(u64, u32)>())
                .sum::<usize>()
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

    pub(crate) fn max_guess(&self) -> Option<u32> {
        self.nodes.iter().map(|n| n.guess).max()
    }
}
