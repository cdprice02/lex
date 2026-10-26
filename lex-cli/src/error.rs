use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum CliError {
    #[error("invalid word length: expected {range:?}, got {got}")]
    InvalidWordLength {
        range: std::ops::RangeInclusive<usize>,
        got: usize,
    },
    #[error("invalid temperature: expected a positive finite number, got {0}")]
    InvalidTemperature(f64),
    #[error("invalid win weight: expected a non-negative finite number, got {0}")]
    InvalidWinWeight(f64),
    #[error("--profile {profile} fixes {flag} to {expected}, got {given}")]
    ProfileConflict {
        profile: lex_data::Profile,
        flag: &'static str,
        given: String,
        expected: String,
    },
}
