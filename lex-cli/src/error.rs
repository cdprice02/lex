use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("invalid word length: expected {range:?}, got {got}")]
    InvalidWordLength {
        range: std::ops::RangeInclusive<usize>,
        got: usize,
    },
}
