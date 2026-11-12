//! `lex word <WORD>... --lang L`: where a word stands on the path from input to Candidate.
//!
//! Reads disk only: nothing is fetched or built, and a cache that is not there reads "not cached"
//! with the later steps unknown.
//!
//! The path in order: shape (one alphabetic token after NFC and lowercasing), a length in the
//! Supported range, Dictionary membership, Corpus frequency, then each Profile's Answer list and
//! Guess pool at that language and length. `lookup` walks it into [`Facts`]; `render` prints them.

mod lookup;
mod render;
#[cfg(test)]
mod tests;

use std::io::IsTerminal as _;

use lex_data::{DataDir, MAX_WORD_LENGTH, MIN_WORD_LENGTH, Profile};

use crate::cli::WordArgs;

pub use lookup::look_up;
pub use render::{render, verdict};

/// Whether a word is in the cached Dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dictionary {
    /// The word never reached this step.
    Skipped,
    /// The language's Dictionary is not cached.
    NotCached,
    /// The Dictionary holds the word.
    Member,
    /// The Dictionary does not hold the word.
    Missing,
}

/// What the cached Corpus says of a word.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Corpus {
    /// The word never reached this step.
    Skipped,
    /// The Corpus of the word's length is not cached.
    NotCached,
    /// The Corpus holds fewer than the least words a Supported length needs.
    Unsupported {
        /// Words the Corpus holds at this length.
        count: usize,
    },
    /// A Supported Corpus that never saw the word.
    Unseen,
    /// A Supported Corpus that holds the word.
    Seen {
        /// Position by frequency, from 1.
        rank: usize,
        /// Words in the Corpus.
        size: usize,
        /// Summed ngrams count.
        frequency: u64,
        /// Probability of being the answer at game start, over the Corpus.
        prior: f64,
    },
}

/// Where a word stands in one Profile's lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileStatus {
    /// The Profile is not fetched.
    NotFetched,
    /// On the Answer list.
    Answer {
        /// Probability of being the answer at game start, over the Answer list; unknown when
        /// the Corpus is not cached.
        prior: Option<f64>,
        /// Words on the Answer list.
        answers: usize,
    },
    /// In the Guess pool only.
    GuessOnly,
    /// In neither list.
    Outside,
}

/// A Profile at the word's language and length, and where the word stands in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileFact {
    /// Which Profile.
    pub profile: Profile,
    /// The word's standing.
    pub status: ProfileStatus,
}

/// Everything known about one word.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The word as typed.
    pub input: String,
    /// The word after NFC and lowercasing.
    pub word: String,
    /// Letters in the normalized word.
    pub length: usize,
    /// One alphabetic token.
    pub shape: bool,
    /// The length is within `MIN_WORD_LENGTH..=MAX_WORD_LENGTH`.
    pub in_range: bool,
    /// Dictionary membership.
    pub dictionary: Dictionary,
    /// What the Corpus says.
    pub corpus: Corpus,
    /// Each Profile at this language and length.
    pub profiles: Vec<ProfileFact>,
}

impl Facts {
    /// A word before any lookup: shape and range decided, everything else not reached.
    fn new(input: &str) -> Self {
        let word = lex_data::normalize(input);
        let length = word.chars().count();
        Self {
            shape: !word.is_empty() && lex_data::is_valid_word(&word),
            in_range: (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&length),
            input: input.to_owned(),
            word,
            length,
            dictionary: Dictionary::Skipped,
            corpus: Corpus::Skipped,
            profiles: Vec::new(),
        }
    }

    /// Whether the lookups apply: a word of one alphabetic token in the Supported range.
    const fn reaches_caches(&self) -> bool {
        self.shape && self.in_range
    }
}

/// `lex word`: looks the words up and prints the report.
///
/// # Errors
///
/// If a cached file cannot be read.
pub fn run(args: &WordArgs) -> anyhow::Result<()> {
    let data = DataDir::new(&args.data.data_dir);
    let facts = look_up(&data, args.lang, &args.words)?;
    let terminal = std::io::stdout().is_terminal();
    anstream::print!("{}", render(&facts, args.lang, terminal));
    Ok(())
}
