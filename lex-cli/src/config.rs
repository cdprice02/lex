//! What one simulation plays, as data: the same value a `simulate` invocation builds from its
//! flags and a [`Track`](crate::tracks::Track) builds from its table row.

use std::path::PathBuf;

use lex_core::Strategy;
use lex_data::{Language, Profile};

/// The words a game is about and the list guesses come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    /// Every Corpus word is both a guess and an answer.
    Corpus {
        /// The Corpus language.
        language: Language,
        /// Length of every word.
        length: usize,
        /// Cap on the Corpus words loaded; `None` loads them all.
        dictionary_length: Option<usize>,
    },
    /// A Profile's answer list and guess pool; it fixes the language and length.
    Profile(Profile),
}

impl Game {
    /// The word length the game is played at.
    #[must_use]
    pub const fn word_length(&self) -> usize {
        match self {
            Self::Corpus { length, .. } => *length,
            Self::Profile(profile) => profile.spec().word_length,
        }
    }
}

/// Which of the game's answers are played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Targets {
    /// Every answer.
    Full,
    /// `n` distinct answers drawn uniformly, reproducible from `seed`.
    /// `n` distinct answers drawn uniformly, reproducible from `seed`.
    Sample {
        /// How many answers.
        n: usize,
        /// Seed of the draw.
        seed: u64,
    },
    /// The words of a file, one per line.
    File(PathBuf),
}

/// Everything a run plays: the game, the Strategy and the targets.
#[derive(Debug, Clone, PartialEq)]
pub struct RunConfig {
    /// The Guess pool and Answer list.
    pub game: Game,
    /// The solver's Strategy.
    pub strategy: Strategy,
    /// Which answers are played.
    pub targets: Targets,
}
