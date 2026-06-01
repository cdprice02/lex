use thiserror::Error;

#[derive(Error, Debug)]
pub enum LexDataError {
    #[error("invalid ngram: {0}")]
    InvalidNgram(String),
    #[error("invalid parse line: {0}")]
    InvalidParseLine(String),
    #[error("invalid word length: expected {expected}, got {got}")]
    WordLengthError { expected: usize, got: usize },
}
