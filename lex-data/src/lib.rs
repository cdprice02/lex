#![cfg_attr(test, feature(test))]
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
mod wiktionary;

pub use cache::{cache_path, get, invalidate, put};
pub use error::LexDataError;
pub use language::Language;
pub use wiktionary::{DictMetadata, fetch_dict, load_metadata, load_valid_words};
pub use word::{Word, WordSet};

pub const MIN_WORD_LENGTH: usize = 3;
pub const MAX_WORD_LENGTH: usize = 10;
