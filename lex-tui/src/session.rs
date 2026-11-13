//! What the UI needs from a game: the seam `lex-cli` implements over its own setup.
//!
//! The trait speaks plain types (words as `String`, feedback as `Vec<Correctness>`), so this crate
//! compiles once and only the implementation is generic over the word length.

use lex_core::Correctness;

/// Why a [`Session`] call failed, in words for the player.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SessionError {
    /// What went wrong.
    message: String,
}

impl SessionError {
    /// An error that says `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// What went wrong.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

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
///
/// Every call can fail; the UI shows the error and carries on.
pub trait Session {
    /// The game being played.
    fn game(&self) -> GameInfo;

    /// The target a seed draws, uniform over the Answer list.
    ///
    /// # Errors
    ///
    /// If the game has no target to draw.
    fn draw(&self, seed: u64) -> Result<String, SessionError>;

    /// The Answer-list word `word` names.
    ///
    /// # Errors
    ///
    /// If `word` is not a word of the game or not on the Answer list.
    fn check_target(&self, word: &str) -> Result<String, SessionError>;

    /// The feedback `guess` earns against `target`.
    ///
    /// # Errors
    ///
    /// If either is not a word of the game.
    fn play(&self, guess: &str, target: &str) -> Result<Vec<Correctness>, SessionError>;

    /// The solver's explanation at the position after `history`; `None` when no guess is
    /// possible.
    ///
    /// # Errors
    ///
    /// If `history` holds a guess that is not a word of the game or has the wrong feedback length.
    fn explain(&mut self, history: &[Played]) -> Result<Option<ExplanationView>, SessionError>;
}
