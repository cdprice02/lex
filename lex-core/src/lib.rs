#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

mod allowed;
mod book;
mod candidates;
mod correctness;
mod endgame;
mod game;
mod guesser;
mod strategy;
mod tree;

/// SHA-256 of every file under `lex-core/src` and `lex-core/Cargo.toml`, sorted by path: which
/// solver source a result was measured against.
pub const SOLVER_DIGEST: &str = env!("LEX_SOLVER_DIGEST");

pub use book::{BOOK_FLOOR, OpeningBook};
pub use candidates::Suggestion;
pub use correctness::{Correctness, WordCorrectness};
pub use game::{GameResult, play};
pub use guesser::{Guess, Guesser};
pub use strategy::{CANONICAL_TEMPERATURE, GuessPool, Strategy, answer_prior};

#[cfg(test)]
mod tests {
    use super::SOLVER_DIGEST;

    #[test]
    fn the_solver_digest_is_a_sha256_hex_string() {
        assert_eq!(SOLVER_DIGEST.len(), 64);
        assert!(SOLVER_DIGEST.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
