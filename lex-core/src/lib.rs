#![cfg_attr(test, feature(test))]
#![feature(variant_count)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

mod candidates;
mod correctness;
mod endgame;
mod game;
mod groups;
mod guesser;
mod strategy;
mod tree;

pub use candidates::Suggestion;
pub use correctness::{Correctness, WordCorrectness};
pub use game::{GameResult, play};
pub use guesser::{Guess, Guesser};
pub use strategy::{CANONICAL_TEMPERATURE, GuessPool, Strategy, answer_prior};
