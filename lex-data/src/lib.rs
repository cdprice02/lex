#![feature(optimize_attribute)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![feature(must_not_suspend)]
#![warn(non_exhaustive_omitted_patterns)]
#![warn(must_not_suspend)]

pub mod blocking;
pub mod language;
pub mod word;

mod cache;
mod error;
mod fetch;
mod parse;

pub use cache::{cache_path, get, invalidate, put};
pub use error::LexDataError;
pub use language::Language;
pub use word::{Word, WordSet};
