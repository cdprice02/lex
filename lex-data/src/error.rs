//! The typed errors of the data layer.

use std::path::PathBuf;

use thiserror::Error;

/// Why loading, building or parsing Corpus, Profile or Opening book data failed.
#[derive(Error, Debug)]
pub enum LexDataError {
    /// An ngram token that is not a single normalized word.
    #[error("invalid ngram: {0}")]
    InvalidNgram(String),
    /// A line that does not follow the V3 ngram line format.
    #[error("invalid parse line: {0}")]
    InvalidParseLine(String),
    /// A word whose length is not the one the caller asked for.
    #[error("invalid word length: expected {expected}, got {got}")]
    WordLengthError {
        /// The length required.
        expected: usize,
        /// The length of the word given.
        got: usize,
    },
    /// A Legacy cache: a file with no header, which must be rebuilt, never read.
    #[error(
        "{path:?} is a legacy or unrecognized cache file (no header); \
         delete it and rebuild with `lex clear --lang <lang>`"
    )]
    LegacyCache {
        /// The rejected file.
        path: PathBuf,
    },
    /// A Cache whose header or record region fails validation.
    #[error(
        "corrupt cache {path:?}: {reason}; delete it and rebuild with `lex clear --lang <lang>`"
    )]
    CorruptCache {
        /// The rejected file.
        path: PathBuf,
        /// What failed.
        reason: String,
    },
    /// A Cache record that is not a valid word and frequency.
    #[error(
        "corrupt cache {path:?}: record {index}: {reason}; delete it and rebuild with `lex clear --lang <lang>`"
    )]
    CorruptRecord {
        /// The rejected file.
        path: PathBuf,
        /// Zero-based index of the record.
        index: usize,
        /// What failed.
        reason: String,
    },
    /// An Opening book file that fails its magic, version, checksum, parse or key check.
    #[error("corrupt opening book {path:?}: {reason}; rebuild it with `lex book build --force`")]
    CorruptBook {
        /// The rejected file.
        path: PathBuf,
        /// What failed.
        reason: String,
    },
    /// A Corpus whose Alphabet has more letters than the solver can encode.
    #[error(
        "the Corpus Alphabet has {size} letters; the solver encodes a letter in one byte and \
         supports at most {max}"
    )]
    AlphabetTooLarge {
        /// Distinct letters in the Corpus.
        size: usize,
        /// The most letters the solver supports.
        max: usize,
    },
    /// A line of a Profile list that fails validation.
    #[error("profile list {file}: line {line}: {reason}")]
    InvalidProfileList {
        /// Name of the list file.
        file: String,
        /// One-based line number.
        line: usize,
        /// What failed.
        reason: String,
    },
    /// A Profile list whose SHA-256 differs from the registry's.
    #[error(
        "profile list {file}: checksum mismatch: expected {expected}, got {actual}; \
         the stored copy or the pinned source changed"
    )]
    ProfileChecksum {
        /// Name of the list file.
        file: String,
        /// The checksum the registry pins.
        expected: String,
        /// The checksum of the list as read.
        actual: String,
    },
}
