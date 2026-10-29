use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LexDataError {
    #[error("invalid ngram: {0}")]
    InvalidNgram(String),
    #[error("invalid parse line: {0}")]
    InvalidParseLine(String),
    #[error("invalid word length: expected {expected}, got {got}")]
    WordLengthError { expected: usize, got: usize },
    #[error(
        "{path:?} is a legacy or unrecognized cache file (no header); \
         delete it and rebuild with `lex clear --lang <lang>`"
    )]
    LegacyCache { path: PathBuf },
    #[error(
        "corrupt cache {path:?}: {reason}; delete it and rebuild with `lex clear --lang <lang>`"
    )]
    CorruptCache { path: PathBuf, reason: String },
    #[error(
        "corrupt cache {path:?}: record {index}: {reason}; delete it and rebuild with `lex clear --lang <lang>`"
    )]
    CorruptRecord {
        path: PathBuf,
        index: usize,
        reason: String,
    },
    #[error("corrupt opening book {path:?}: {reason}; rebuild it with `lex book build --force`")]
    CorruptBook { path: PathBuf, reason: String },
    #[error(
        "the Corpus Alphabet has {size} letters; the solver encodes a letter in one byte and \
         supports at most {max}"
    )]
    AlphabetTooLarge { size: usize, max: usize },
    #[error("profile list {file}: line {line}: {reason}")]
    InvalidProfileList {
        file: String,
        line: usize,
        reason: String,
    },
    #[error(
        "profile list {file}: checksum mismatch: expected {expected}, got {actual}; \
         the stored copy or the pinned source changed"
    )]
    ProfileChecksum {
        file: String,
        expected: String,
        actual: String,
    },
}
