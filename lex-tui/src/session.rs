//! What the UI needs from a game: the seam `lex-cli` implements over its own setup.
//!
//! The trait speaks plain types (words as `String`, feedback as `Vec<Correctness>`), so this crate
//! compiles once and only the implementation is generic over the word length.

use lex_core::Correctness;

/// A guess played and the feedback it earned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played {
    /// The guess.
    pub word: String,
    /// The feedback, one entry per letter.
    pub feedback: Vec<Correctness>,
}

impl Played {
    /// Whether every letter is [`Correctness::Correct`].
    #[must_use]
    pub fn is_correct(&self) -> bool {
        self.feedback.iter().all(|&c| c == Correctness::Correct)
    }
}

/// The game being watched, for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// The Profile or language name.
    pub name: String,
    /// Letters per word.
    pub word_length: usize,
}

/// One guess the solver weighed.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The guess.
    pub word: String,
    /// Its expected information, in bits.
    pub bits: f64,
    /// Its Expected remaining.
    pub remaining: f64,
    /// Its Guess cost, where the technique computed one.
    pub cost: Option<f64>,
    /// Whether it can still be the answer.
    pub in_candidates: bool,
}

/// Why the solver chose its guess at a position: the explanation panel's content.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplanationView {
    /// The guess the solver plays.
    pub guess: String,
    /// The technique that chose it ("Rollout"), when the solver explained.
    pub technique: Option<String>,
    /// Whether the position came from the Opening book.
    pub from_book: bool,
    /// How many words are still possible answers.
    pub candidates: usize,
    /// The guesses weighed, the chosen one first.
    pub rows: Vec<Row>,
}

/// A game the Watch mode can play. The solver call, [`Session::explain`], is slow; the rest are
/// quick.
pub trait Session {
    /// The game being played.
    fn game(&self) -> GameInfo;

    /// The target a seed draws, uniform over the Answer list.
    fn draw(&self, seed: u64) -> String;

    /// The Answer-list word `word` names, or why it is not one.
    ///
    /// # Errors
    ///
    /// A message for the player when `word` is not on the Answer list.
    fn check_target(&self, word: &str) -> Result<String, String>;

    /// The feedback `guess` earns against `target`.
    fn play(&self, guess: &str, target: &str) -> Vec<Correctness>;

    /// The solver's explanation at the position after `history`; `None` when no guess is
    /// possible.
    fn explain(&mut self, history: &[Played]) -> Option<ExplanationView>;
}
