#![cfg_attr(test, feature(test))]
#![feature(optimize_attribute)]
#![feature(variant_count)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

mod correctness;
mod game;
mod guesser;

pub use correctness::{Correctness, WordCorrectness};
pub use game::{GameResult, play};
pub use guesser::{Guess, Guesser};
