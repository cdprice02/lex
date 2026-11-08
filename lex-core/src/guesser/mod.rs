//! The per-game solver state: guesses played and a cursor into the shared Decision tree.

use std::cell::RefCell;

use lex_data::Word;
use lex_data::WordSet;

use crate::allowed::HardRule;
use crate::book::{BOOK_FLOOR, OpeningBook};
use crate::candidates::{Candidates, Suggestion};
use crate::correctness::WordCorrectness;
use crate::strategy::{GuessPool, Pool, Strategy};
use crate::tree::{DecisionTree, NodeId};

/// One guess and the feedback it earned.
#[derive(Debug, Clone)]
pub struct Guess<const N: usize> {
    /// The word played.
    word: Word<N>,
    /// The feedback the word earned.
    correctness: WordCorrectness<N>,
}

impl<const N: usize> Guess<N> {
    /// A guess of `word` that earned `correctness`.
    #[must_use]
    pub const fn new(word: Word<N>, correctness: WordCorrectness<N>) -> Self {
        Self { word, correctness }
    }

    /// The word played.
    #[must_use]
    pub const fn word(&self) -> Word<N> {
        self.word
    }

    /// The feedback the word earned.
    #[must_use]
    pub const fn correctness(&self) -> WordCorrectness<N> {
        self.correctness
    }
}

/// One game's solver state: the guesses played and a cursor into a shared decision tree.
///
/// The tree can be reused across games with [`reset`](Self::reset), so repeated positions are
/// scored once. The [`Strategy`] is fixed at construction.
pub struct Guesser<const N: usize> {
    /// The Decision tree, reused across this guesser's games and owned by it alone. A `RefCell`
    /// because the memoizing queries (`suggest`, `build_book`) take `&self`; a borrow never spans
    /// a call into another `Guesser` method.
    tree: RefCell<DecisionTree<N>>,
    /// The current position in the tree.
    node: NodeId,
    /// The guesses of this game, in order.
    history: Vec<Guess<N>>,
}

impl<const N: usize> Guesser<N> {
    /// `pool` is every word the solver may guess; `answers` are the only words the target can
    /// be, a subset of `pool` (all of it when nothing narrows the answers).
    ///
    /// # Panics
    ///
    /// If an answer is not in the pool, if the strategy's temperature is not positive and
    /// finite, or if its win weight is negative or not finite.
    #[must_use]
    pub fn new(pool: &WordSet<N>, answers: &[Word<N>], strategy: Strategy) -> Self {
        Self::build(pool, answers, strategy, None)
    }

    /// Like [`new`](Self::new), seeded with an [`OpeningBook`] built for the same pool, Answer
    /// list and strategy: a position on the book's path takes its booked guess without a search,
    /// and a guess off the path leaves the book, with everything below computed online. Play is
    /// identical to an unseeded guesser's.
    ///
    /// # Panics
    ///
    /// As [`new`](Self::new), and if the book names a guess outside the pool.
    #[must_use]
    pub fn with_book(
        pool: &WordSet<N>,
        answers: &[Word<N>],
        strategy: Strategy,
        book: OpeningBook<N>,
    ) -> Self {
        Self::build(pool, answers, strategy, Some(book))
    }

    /// Shared constructor: validates the Strategy and book, then roots a fresh tree.
    fn build(
        pool: &WordSet<N>,
        answers: &[Word<N>],
        strategy: Strategy,
        book: Option<OpeningBook<N>>,
    ) -> Self {
        strategy.assert_valid();
        let pool = Pool::new(pool);
        assert!(
            book.as_ref()
                .and_then(OpeningBook::max_guess)
                .is_none_or(|g| pool.indices().contains(&g)),
            "the opening book names a guess outside the pool"
        );
        let candidates = Candidates::new(pool, answers);
        Self {
            tree: RefCell::new(DecisionTree::new(candidates, strategy, book)),
            node: DecisionTree::<N>::ROOT,
            history: Vec::new(),
        }
    }

    /// Builds the [`OpeningBook`] of this guesser's starting position: every position on the
    /// policy's paths whose work exceeds [`BOOK_FLOOR`].
    #[must_use]
    pub fn build_book(&self) -> OpeningBook<N> {
        self.build_book_reporting(&mut |_| {})
    }

    /// As [`build_book`](Self::build_book), calling `on_booked` with the number of positions
    /// booked so far each time one is added, so a caller can show progress on a long build.
    pub fn build_book_reporting(&self, on_booked: &mut dyn FnMut(usize)) -> OpeningBook<N> {
        self.tree.borrow_mut().build_book(BOOK_FLOOR, on_booked)
    }

    /// As [`build_book`](Self::build_book) with a chosen floor, in pattern evaluations (guesses
    /// the strategy allows times Candidate set size).
    #[must_use]
    pub fn build_book_with_floor(&self, floor: u64) -> OpeningBook<N> {
        self.tree.borrow_mut().build_book(floor, &mut |_| {})
    }

    /// A guesser already holding `history`, as if those guesses had been pushed in order.
    #[must_use]
    pub fn with_history(
        pool: &WordSet<N>,
        answers: &[Word<N>],
        strategy: Strategy,
        history: Vec<Guess<N>>,
    ) -> Self {
        let mut guesser = Self::new(pool, answers, strategy);
        for guess in history {
            guesser.push_guess(guess);
        }
        guesser
    }

    /// Forgets the guesses played and returns to the starting position. The decision tree, and
    /// every suggestion already computed in it, is kept.
    pub fn reset(&mut self) {
        self.node = DecisionTree::<N>::ROOT;
        self.history.clear();
    }

    /// The guesses played so far, in order.
    #[must_use]
    pub fn history(&self) -> &[Guess<N>] {
        &self.history
    }

    /// Records `guess` and moves to the position it leads to.
    pub fn push_guess(&mut self, guess: Guess<N>) {
        self.node = self.tree.borrow_mut().child(self.node, &guess);
        self.history.push(guess);
    }

    /// Number of candidate words still consistent with the pushed guesses.
    #[must_use]
    pub fn num_candidates(&self) -> usize {
        self.tree.borrow().candidates(self.node).len()
    }

    /// The `k` best guesses by the strategy's score, descending. Equal scores prefer words still
    /// in the candidate set, then frequency order, so the first element always agrees with
    /// [`suggest`](Self::suggest). Not memoized.
    #[must_use]
    pub fn suggest_top_k(&self, k: usize) -> Vec<Suggestion<N>> {
        let tree = self.tree.borrow();
        let candidates = tree.candidates(self.node);
        let pool = candidates.pool();
        candidates
            .suggest_top_k(tree.strategy(), tree.allowed(self.node), k)
            .iter()
            .map(|scored| scored.suggestion(&pool))
            .collect()
    }

    /// Why `word` may not be played next under the strategy's pool rule and the guesses pushed,
    /// as a one-line reason, or `None` when it is allowed (always, outside hard and ultrahard
    /// mode). Independent of the tree, so it also vets a word the solver never scored.
    #[must_use]
    pub fn rule_violation(&self, word: Word<N>) -> Option<String> {
        let letters = |w: &Word<N>| -> [char; N] { std::array::from_fn(|i| w[i]) };
        let candidate = letters(&word);
        match self.tree.borrow().strategy().pool {
            GuessPool::All => None,
            GuessPool::Hard => self.history.iter().find_map(|guess| {
                HardRule::new(&letters(&guess.word()), guess.correctness().ordinal())
                    .violation(&candidate)
                    .map(|v| format!("hard mode: {v}"))
            }),
            GuessPool::Ultrahard => self
                .history
                .iter()
                .find(|g| WordCorrectness::correct(word, g.word()) != g.correctness())
                .map(|g| {
                    format!(
                        "ultrahard mode: must be consistent with the feedback for {}",
                        g.word()
                    )
                }),
        }
    }

    /// The best next guess under the Strategy, or `None` when no word is consistent.
    #[must_use]
    pub fn suggest(&self) -> Option<Word<N>> {
        self.tree.borrow_mut().suggest(self.node)
    }
}

/// The `n`-th letter of `a..z`, wrapping, for fixtures.
#[cfg(any(test, feature = "bench"))]
pub fn letter(n: u64) -> char {
    char::from(b'a' + u8::try_from(n % 26).unwrap_or_default())
}

/// A guesser whose answers are every pool word.
#[cfg(any(test, feature = "bench"))]
fn corpus_guesser<const N: usize>(pool: &WordSet<N>, strategy: Strategy) -> Guesser<N> {
    let answers: Vec<_> = pool.words().collect();
    Guesser::new(pool, &answers, strategy)
}

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches;

#[cfg(test)]
mod tests;
