use thiserror::Error;

#[derive(Error, Debug)]
pub enum LexError {
    #[error("invalid word length: expected {range:?}, got {got}")]
    UnexpectedWordLength {
        range: std::ops::RangeInclusive<usize>,
        got: usize,
    },
}
