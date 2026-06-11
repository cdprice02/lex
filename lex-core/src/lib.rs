#![cfg_attr(test, feature(test))]
#![feature(optimize_attribute)]
#![feature(variant_count)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

pub mod error;
pub mod game;
pub mod guesser;
