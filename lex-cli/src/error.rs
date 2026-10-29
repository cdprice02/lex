//! Errors for rejected command line values.

use thiserror::Error;

/// Why a command line value is rejected.
#[derive(Error, Debug)]
pub enum CliError {
    /// A word length outside the supported range.
    #[error("invalid word length: expected {range:?}, got {got}")]
    InvalidWordLength {
        /// The supported lengths.
        range: std::ops::RangeInclusive<usize>,
        /// The length given.
        got: usize,
    },
    /// A temperature that is not positive and finite.
    #[error("invalid temperature: expected a positive finite number, got {0}")]
    InvalidTemperature(f64),
    /// A win weight that is negative or not finite.
    #[error("invalid win weight: expected a non-negative finite number, got {0}")]
    InvalidWinWeight(f64),
    /// A Track id that names no Track.
    #[error("unknown track {0:?}")]
    UnknownTrack(String),
}
